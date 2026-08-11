use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool, types::Json};
use thiserror::Error;
use time::{Duration, OffsetDateTime, UtcOffset};
use uuid::Uuid;

use super::workflow::AuctionFormat;
use super::{
    TenantScope,
    idempotency::{
        self, AcquireIdempotency, AcquireIdempotencyOutcome, CompleteIdempotency,
        CompleteIdempotencyOutcome, IdempotencyPrincipalType, IdempotencyRepositoryError,
        IdempotentResponse, PreflightIdempotency, PreflightIdempotencyOutcome,
    },
    mmr::{self, MmrLaneKey, MmrRepositoryError},
    notification_outbox::{
        self, AUCTION_MATCH_LINEUP_CONFIRMATION_CREATE_TYPE,
        AUCTION_MATCH_LINEUP_CONFIRMATION_SCHEMA_VERSION, AUCTION_RECRUITMENT_MESSAGE_CREATE_TYPE,
        AUCTION_RECRUITMENT_MESSAGE_DELETE_TYPE, AUCTION_RECRUITMENT_MESSAGE_SCHEMA_VERSION,
        AUCTION_RECRUITMENT_MESSAGE_UPDATE_TYPE,
        AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE_TYPE,
        AUCTION_TOURNAMENT_RESULT_CREATE_TYPE, AUCTION_TOURNAMENT_RESULT_DELETE_SCHEMA_VERSION,
        AUCTION_TOURNAMENT_RESULT_DELETE_TYPE, AUCTION_TOURNAMENT_RESULT_SCHEMA_VERSION,
        ENTRY_CONFIRMATION_CREATE_TYPE, ENTRY_CONFIRMATION_SCHEMA_VERSION, EnqueueNotification,
        EnqueueNotificationOutcome, NotificationOutboxError, RECRUITMENT_MESSAGE_CREATE_TYPE,
        RECRUITMENT_MESSAGE_DELETE_TYPE, RECRUITMENT_MESSAGE_SCHEMA_VERSION,
        RECRUITMENT_MESSAGE_UPDATE_TYPE, RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE_TYPE,
        RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_SCHEMA_VERSION, SERIES_RESULT_CREATE_TYPE,
        SERIES_RESULT_SCHEMA_VERSION,
    },
    riot::{
        self, CanonicalRiotAccount, RiotAccountLinkState, RiotAccountRecord, RiotRepositoryError,
    },
};
use crate::{
    auth::AuthService,
    domain::{
        AuctionMatchId, AuctionRecruitmentId, AuctionTournamentId, AuctionTournamentStatus,
        DiscordChannelId, DiscordInteractionId, DiscordMessageId, DiscordRoleId, DiscordUserId,
        GameId, GuildId, GuildMemberStatus, GuildStatus, IdempotencyRecordId, Lane, LaneMatchup,
        MatchupPlayer, NotificationId, OutboxEventId, RecruitmentId, RecruitmentStatus, RosterTeam,
        SeasonId, SeriesId, SeriesStatus, UserId, apply_game_elo,
    },
    infrastructure::postgres::{
        DatabaseTransaction, DeadlockRetryPolicy, TransactionAttemptError, TransactionRetryError,
        begin, run_with_deadlock_retry,
    },
    integrations::riot::RiotRecords,
    state::RequestId,
};

const BOT_PRINCIPAL_ID: &str = "monkeybot-discord-bot";
const IDEMPOTENCY_TTL: Duration = Duration::hours(24);
const RECRUITMENT_TARGET_COUNT: i16 = 10;
const MAX_PREFERRED_LANES: usize = 5;
const SETUP_METHOD: &str = "POST";
const SETUP_ROUTE: &str = "/internal/guilds/{guildId}/setup";
const REGISTRATION_METHOD: &str = "POST";
const REGISTRATION_ROUTE: &str = "/internal/guilds/{guildId}/registrations";
const SERIES_CREATE_METHOD: &str = "POST";
const SERIES_CREATE_ROUTE: &str = "/api/guilds/{guildId}/series";
const ENTRY_DRAFT_AUCTION_CONVERT_ROUTE: &str =
    "/api/guilds/{guildId}/recruitments/{id}/auction-recruitment";
const SERIES_REVERT_METHOD: &str = "POST";
const SERIES_REVERT_ROUTE: &str = "/api/guilds/{guildId}/series/{id}/revert";
const SERIES_DELETE_METHOD: &str = "DELETE";
const SERIES_DELETE_ROUTE: &str = "/api/guilds/{guildId}/series/{id}";
const SERIES_GAME_SUBMIT_METHOD: &str = "POST";
const SERIES_GAME_SUBMIT_ROUTE: &str = "/api/guilds/{guildId}/series/{id}/games";
const SERIES_GAME_REVERT_METHOD: &str = "DELETE";
const SERIES_GAME_REVERT_ROUTE: &str = "/api/guilds/{guildId}/series/{id}/games/last";
const SERIES_EARLY_COMPLETE_METHOD: &str = "POST";
const SERIES_EARLY_COMPLETE_ROUTE: &str = "/api/guilds/{guildId}/series/{id}/complete";
const AUCTION_RECRUITMENT_CREATE_ROUTE: &str = "/api/guilds/{guildId}/auction-recruitments";
const AUCTION_RECRUITMENT_CLOSE_ROUTE: &str =
    "/api/guilds/{guildId}/auction-recruitments/{id}/close";
const AUCTION_RECRUITMENT_CANCEL_ROUTE: &str = "/api/guilds/{guildId}/auction-recruitments/{id}";
const AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DMS_ROUTE: &str =
    "/api/guilds/{guildId}/auction-recruitments/{id}/participant-confirmation-dms";
const AUCTION_RECRUITMENT_FORCE_CANCEL_METHOD: &str = "POST";
const AUCTION_RECRUITMENT_FORCE_CANCEL_ROUTE: &str =
    "/api/guilds/{guildId}/auction-recruitments/{id}/force-cancel";
const AUCTION_RECRUITMENT_MEMBERS_ROUTE: &str =
    "/api/guilds/{guildId}/auction-recruitments/{id}/members";
const AUCTION_RECRUITMENT_MEMBER_ROUTE: &str =
    "/api/guilds/{guildId}/auction-recruitments/{id}/members/{memberId}";
const AUCTION_SETUP_DRAFT_ROUTE: &str =
    "/api/guilds/{guildId}/auction-recruitments/{id}/setup-draft";
const AUCTION_TOURNAMENT_CREATE_ROUTE: &str = "/api/guilds/{guildId}/auction-tournaments";
const AUCTION_TOURNAMENT_CANCEL_METHOD: &str = "POST";
const AUCTION_TOURNAMENT_CANCEL_ROUTE: &str =
    "/api/guilds/{guildId}/auction-tournaments/{id}/cancel";
const AUCTION_TOURNAMENT_BRACKET_COMMAND_ROUTE: &str =
    "/api/guilds/{guildId}/auction-tournaments/{id}/bracket/commands";
const AUCTION_MATCH_LINEUP_ROUTE: &str = "/api/guilds/{guildId}/auction-matches/{matchId}/lineup";
const AUCTION_MATCH_LINEUP_LOCK_ROUTE: &str =
    "/api/guilds/{guildId}/auction-matches/{matchId}/lineup/lock";
const AUCTION_MATCH_LINEUP_UNLOCK_ROUTE: &str =
    "/api/guilds/{guildId}/auction-matches/{matchId}/lineup/unlock";
const AUCTION_MATCH_GAME_SUBMIT_ROUTE: &str =
    "/api/guilds/{guildId}/auction-matches/{matchId}/games";
const AUCTION_MATCH_GAME_REVERT_ROUTE: &str =
    "/api/guilds/{guildId}/auction-matches/{matchId}/games/last";
const RECRUITMENT_PARTICIPANT_CONFIRMATION_DMS_ROUTE: &str =
    "/api/guilds/{guildId}/recruitments/{id}/participant-confirmation-dms";
const AUCTION_INITIAL_TEAM_POINTS: i32 = 1_000;
pub const DEFAULT_RECRUITMENT_BEST_OF: i16 = 3;
const MAX_CHAMPION_ID_BYTES: usize = 64;
const MAX_PATCH_VERSION_BYTES: usize = 32;
const MAX_AUCTION_TEAM_NAME_BYTES: usize = 64;
const MAX_AUCTION_TOURNAMENT_CANCEL_REASON_CHARS: usize = 240;
const AUCTION_BRACKET_SLOT_KEYS: [&str; 4] = ["A_0", "A_1", "B_0", "B_1"];
const AUCTION_TOURNAMENT_ELIGIBLE_RECORDS_CTE: &str = "WITH eligible AS (
    SELECT game.id,
           game.played_at,
           participant.lane,
           participant.champion_id,
           (
               (participant.team = 'TEAM_1' AND (
                   (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
                   (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
               )) OR
               (participant.team = 'TEAM_2' AND (
                   (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
                   (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
               ))
           ) AS won
    FROM games AS game
    JOIN game_participants AS participant
      ON participant.guild_id = game.guild_id
     AND participant.game_id = game.id
     AND participant.user_id = $3
    WHERE game.guild_id = $1
      AND game.season_id = $2
      AND game.status = 'CONFIRMED'
      AND (
          (game.series_id IS NOT NULL AND EXISTS (
              SELECT 1
              FROM series AS parent_series
              WHERE parent_series.guild_id = game.guild_id
                AND parent_series.id = game.series_id
                AND parent_series.status <> 'CANCELLED'
                AND parent_series.deleted_at IS NULL
          ))
          OR
          (game.auction_match_id IS NOT NULL AND EXISTS (
              SELECT 1
              FROM auction_matches AS parent_match
              JOIN auction_tournaments AS parent_tournament
                ON parent_tournament.guild_id = parent_match.guild_id
               AND parent_tournament.id = parent_match.tournament_id
              WHERE parent_match.guild_id = game.guild_id
                AND parent_match.id = game.auction_match_id
                AND parent_match.status <> 'CANCELLED'
                AND parent_match.deleted_at IS NULL
                AND parent_tournament.status <> 'CANCELLED'
                AND parent_tournament.deleted_at IS NULL
          ))
      )
)";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotActor {
    pub id: DiscordUserId,
    pub global_display_name: String,
    pub display_name: String,
    pub avatar_hash: Option<String>,
    pub role_ids: Vec<DiscordRoleId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupGuildCommand {
    pub actor: BotActor,
    pub guild_name: String,
    pub guild_icon_hash: Option<String>,
    pub operator_role_id: DiscordRoleId,
    pub recruitment_channel_id: DiscordChannelId,
    pub entry_channel_id: Option<DiscordChannelId>,
    pub result_channel_id: Option<DiscordChannelId>,
    pub log_channel_id: Option<DiscordChannelId>,
    pub season_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotSeasonSnapshot {
    pub id: SeasonId,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupGuildResult {
    pub guild_id: GuildId,
    pub display_name: String,
    pub operator_role_id: DiscordRoleId,
    pub current_season: BotSeasonSnapshot,
    pub config_revision: i64,
    pub dashboard_href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterGuildMemberInput {
    pub actor: BotActor,
    pub requested_game_name: String,
    pub requested_tag_line: String,
    pub requested_platform: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterGuildMemberCommand {
    pub input: RegisterGuildMemberInput,
    pub account: CanonicalRiotAccount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RegistrationState {
    Created,
    Existing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredGuildMemberSnapshot {
    pub id: DiscordUserId,
    pub display_name: String,
    pub status: GuildMemberStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredRiotAccountSnapshot {
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
    pub platform: String,
    pub is_main: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterGuildMemberResult {
    pub member: RegisteredGuildMemberSnapshot,
    pub riot_account: RegisteredRiotAccountSnapshot,
    pub registration: RegistrationState,
    pub account_link: RegistrationState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BotRecruitmentCommand {
    Create {
        actor: BotActor,
        rules: RecruitmentSeriesRules,
        #[serde(with = "time::serde::rfc3339")]
        expected_start_at: OffsetDateTime,
    },
    BindMessage {
        actor: BotActor,
        recruitment_id: RecruitmentId,
        channel_id: DiscordChannelId,
        message_id: DiscordMessageId,
    },
    Join {
        actor: BotActor,
        recruitment_id: RecruitmentId,
        participant: BotActor,
    },
    Leave {
        actor: BotActor,
        recruitment_id: RecruitmentId,
        user_id: UserId,
    },
    SetPreferredLanes {
        actor: BotActor,
        recruitment_id: RecruitmentId,
        user_id: UserId,
        lanes: Vec<Lane>,
    },
    Close {
        actor: BotActor,
        recruitment_id: RecruitmentId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BotAuctionRecruitmentParticipationCommand {
    Join {
        actor: BotActor,
        participant: BotActor,
        recruitment_id: AuctionRecruitmentId,
        declared_lane: Option<Lane>,
    },
    Leave {
        actor: BotActor,
        recruitment_id: AuctionRecruitmentId,
        user_id: UserId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecruitmentSeriesRules {
    pub best_of: i16,
    pub hard_fearless: bool,
}

impl Default for RecruitmentSeriesRules {
    fn default() -> Self {
        Self {
            best_of: DEFAULT_RECRUITMENT_BEST_OF,
            hard_fearless: true,
        }
    }
}

impl RecruitmentSeriesRules {
    pub fn from_optional(best_of: Option<i16>, hard_fearless: Option<bool>) -> Self {
        Self {
            best_of: best_of.unwrap_or(DEFAULT_RECRUITMENT_BEST_OF),
            hard_fearless: hard_fearless.unwrap_or(true),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionRecruitmentSeriesRules {
    pub hard_fearless: bool,
    pub semifinal_best_of: Option<i16>,
    pub final_best_of: i16,
}

impl AuctionRecruitmentSeriesRules {
    pub fn from_optional(
        format: AuctionFormat,
        semifinal_best_of: Option<i16>,
        final_best_of: Option<i16>,
        hard_fearless: Option<bool>,
    ) -> Self {
        Self {
            hard_fearless: hard_fearless.unwrap_or(true),
            semifinal_best_of: match format {
                AuctionFormat::TenPerson => semifinal_best_of,
                AuctionFormat::TwentyPerson => {
                    Some(semifinal_best_of.unwrap_or(DEFAULT_RECRUITMENT_BEST_OF))
                }
            },
            final_best_of: final_best_of.unwrap_or(DEFAULT_RECRUITMENT_BEST_OF),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecruitmentSummarySnapshot {
    pub id: RecruitmentId,
    pub display_number: i64,
    pub title: String,
    pub status: RecruitmentStatus,
    pub target_count: i16,
    pub best_of: i16,
    pub hard_fearless: bool,
    pub revision: i64,
    pub channel_id: Option<DiscordChannelId>,
    pub message_id: Option<DiscordMessageId>,
    pub created_by_id: UserId,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecruitmentParticipantSnapshot {
    pub id: UserId,
    pub display_name: String,
    pub preferred_lanes: Vec<Lane>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecruitmentLinksSnapshot {
    pub entry: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecruitmentSnapshot {
    pub recruitment: RecruitmentSummarySnapshot,
    pub participants: Vec<RecruitmentParticipantSnapshot>,
    pub links: RecruitmentLinksSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionRecruitmentSummarySnapshot {
    pub id: AuctionRecruitmentId,
    pub display_number: i64,
    pub title: String,
    pub format: AuctionFormat,
    pub rules: AuctionRecruitmentSeriesRules,
    pub participant_count: i16,
    pub joined_participant_count: i64,
    pub team_count: i16,
    pub status: RecruitmentStatus,
    pub revision: i64,
    pub channel_id: Option<DiscordChannelId>,
    pub message_id: Option<DiscordMessageId>,
    pub created_by_id: UserId,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionRecruitmentParticipantSnapshot {
    pub id: UserId,
    pub display_name: String,
    pub declared_lane: Option<Lane>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionRecruitmentLinksSnapshot {
    pub draft: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionRecruitmentPermissions {
    pub can_manage_members: bool,
    pub can_close: bool,
    pub can_send_participant_confirmation_dms: bool,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionRecruitmentSnapshot {
    pub recruitment: AuctionRecruitmentSummarySnapshot,
    pub participants: Vec<AuctionRecruitmentParticipantSnapshot>,
    pub links: AuctionRecruitmentLinksSnapshot,
    pub permissions: AuctionRecruitmentPermissions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionSetupDraftTeamInput {
    pub team_index: i16,
    pub team_name: String,
    pub captain_user_id: UserId,
    pub initial_points: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionSetupDraftTeamSnapshot {
    pub team_index: i16,
    pub team_name: String,
    pub captain_user_id: Option<UserId>,
    pub initial_points: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionSetupDraftStateSnapshot {
    pub recruitment_id: AuctionRecruitmentId,
    pub revision: i64,
    pub team_count: i16,
    pub teams: Vec<AuctionSetupDraftTeamSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionSetupDraftPermissions {
    pub can_edit: bool,
    pub can_create_tournament: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionSetupDraftSnapshot {
    pub draft: AuctionSetupDraftStateSnapshot,
    pub participants: Vec<AuctionRecruitmentParticipantSnapshot>,
    pub permissions: AuctionSetupDraftPermissions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentDraftSummarySnapshot {
    pub id: AuctionTournamentId,
    pub source_recruitment_id: AuctionRecruitmentId,
    pub display_number: i64,
    pub title: String,
    pub format: AuctionFormat,
    pub rules: AuctionRecruitmentSeriesRules,
    pub status: AuctionTournamentStatus,
    pub revision: i64,
    pub href: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentTeamMemberSnapshot {
    pub team_member_id: Uuid,
    pub user_id: UserId,
    pub display_name: String,
    pub declared_lane: Option<Lane>,
    pub mmr_at_acquisition: Option<f64>,
    pub snapshot_origin: String,
    pub acquired_via: String,
    pub roster_slot: i16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentTeamSnapshot {
    pub id: Uuid,
    pub team_index: i16,
    pub team_name: String,
    pub initial_points: i32,
    pub current_points: i32,
    pub point_revision: i64,
    pub members: Vec<AuctionTournamentTeamMemberSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentCandidateSnapshot {
    pub user_id: UserId,
    pub display_name: String,
    pub declared_lane: Option<Lane>,
    pub mmr_at_start: Option<f64>,
    pub snapshot_origin: String,
    pub queue_position: i64,
    pub state: String,
    pub draw_count: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentCandidateRecordSummarySnapshot {
    pub games: i32,
    pub wins: i32,
    pub losses: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentCandidateLaneMmrSnapshot {
    pub lane: Lane,
    pub mmr: f64,
    pub games: i32,
    pub wins: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentCandidateChampionRecordSnapshot {
    pub champion_id: String,
    pub games: i32,
    pub wins: i32,
    pub losses: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentCandidateRecentGameSnapshot {
    pub id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub played_at: OffsetDateTime,
    pub result: String,
    pub lane: Lane,
    pub champion_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentCurrentTargetRiotSnapshot {
    pub games: i32,
    pub wins: i32,
    pub champion_ids: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub refreshed_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentCurrentTargetProfileSnapshot {
    pub avatar_url: Option<String>,
    pub profile_icon_id: Option<i32>,
    pub riot_tier: Option<String>,
    pub main_lane: Option<Lane>,
    pub summary: AuctionTournamentCandidateRecordSummarySnapshot,
    pub lane_mmr: Vec<AuctionTournamentCandidateLaneMmrSnapshot>,
    pub champion_ids: Vec<String>,
    pub champion_records: Vec<AuctionTournamentCandidateChampionRecordSnapshot>,
    pub recent_games: Vec<AuctionTournamentCandidateRecentGameSnapshot>,
    pub riot: Option<AuctionTournamentCurrentTargetRiotSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentRecentActionSnapshot {
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentDraftStateSnapshot {
    pub current_target: Option<AuctionTournamentCandidateSnapshot>,
    pub current_target_profile: Option<AuctionTournamentCurrentTargetProfileSnapshot>,
    pub unsold: Vec<AuctionTournamentCandidateSnapshot>,
    pub recent_action: Option<AuctionTournamentRecentActionSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentDraftPermissions {
    pub can_manage_draft: bool,
    pub can_manage_bracket: bool,
    pub can_revert_last: bool,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentDraftSnapshot {
    pub tournament: AuctionTournamentDraftSummarySnapshot,
    pub teams: Vec<AuctionTournamentTeamSnapshot>,
    pub candidates: Vec<AuctionTournamentCandidateSnapshot>,
    pub draft: AuctionTournamentDraftStateSnapshot,
    pub permissions: AuctionTournamentDraftPermissions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentBracketSummarySnapshot {
    pub id: AuctionTournamentId,
    pub title: String,
    pub display_number: i64,
    pub format: AuctionFormat,
    pub rules: AuctionRecruitmentSeriesRules,
    pub status: AuctionTournamentStatus,
    pub revision: i64,
    pub href: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentBracketTeamSnapshot {
    pub id: Uuid,
    pub team_index: i16,
    pub team_name: String,
    pub initial_points: i32,
    pub current_points: i32,
    pub members: Vec<AuctionTournamentTeamMemberSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentBracketSlotSnapshot {
    pub slot_key: String,
    pub team_id: Option<Uuid>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentBracketMatchSnapshot {
    pub id: Uuid,
    pub match_key: String,
    pub best_of: i16,
    pub hard_fearless: bool,
    pub left_team_id: Option<Uuid>,
    pub right_team_id: Option<Uuid>,
    pub status: String,
    pub revision: i64,
    pub left_score: i64,
    pub right_score: i64,
    pub winner_team_id: Option<Uuid>,
    pub lineups_locked: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentBracketStateSnapshot {
    pub locked: bool,
    pub slots: Vec<AuctionTournamentBracketSlotSnapshot>,
    pub matches: Vec<AuctionTournamentBracketMatchSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentBracketPermissions {
    pub can_manage_bracket: bool,
    pub can_manage_matches: bool,
    pub can_recover_bracket: bool,
    pub can_revert_draft: bool,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentBracketSnapshot {
    pub tournament: AuctionTournamentBracketSummarySnapshot,
    pub teams: Vec<AuctionTournamentBracketTeamSnapshot>,
    pub bracket: AuctionTournamentBracketStateSnapshot,
    pub permissions: AuctionTournamentBracketPermissions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionMatchSnapshotMatch {
    pub id: AuctionMatchId,
    pub tournament_id: AuctionTournamentId,
    pub match_key: String,
    pub label: String,
    pub best_of: i16,
    pub hard_fearless: bool,
    pub status: String,
    pub revision: i64,
    pub left_team_id: Uuid,
    pub right_team_id: Uuid,
    pub left_score: i64,
    pub right_score: i64,
    pub winner_team_id: Option<Uuid>,
    pub lineups_locked: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionMatchSnapshotTeam {
    pub id: Uuid,
    pub team_name: String,
    pub members: Vec<AuctionTournamentTeamMemberSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionMatchLineupAssignmentSnapshot {
    pub team_id: Uuid,
    pub lane: Lane,
    pub team_member_id: Uuid,
    pub user_id: UserId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionMatchConfirmedGameSnapshot {
    pub id: GameId,
    pub set_number: i16,
    pub side_preset: String,
    pub winner: String,
    pub picks: BTreeMap<String, Option<String>>,
    pub bans: SeriesPickBanBansSnapshot,
    pub patch_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionMatchSnapshotPermissions {
    pub can_manage_lineup: bool,
    pub can_enter_results: bool,
    pub can_unlock_lineup: bool,
    pub can_revert_last_game: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionMatchSnapshot {
    pub r#match: AuctionMatchSnapshotMatch,
    pub teams: Vec<AuctionMatchSnapshotTeam>,
    pub lineups: Vec<AuctionMatchLineupAssignmentSnapshot>,
    pub games: Vec<AuctionMatchConfirmedGameSnapshot>,
    pub permissions: AuctionMatchSnapshotPermissions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuctionMatchLineupAssignmentCommand {
    pub expected_revision: i64,
    pub team_id: Uuid,
    pub lane: Lane,
    pub team_member_id: Option<Uuid>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuctionMatchLineupLockCommand {
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuctionMatchGameSubmitCommand {
    pub expected_revision: i64,
    pub set_number: i16,
    pub side_preset: String,
    pub winner: String,
    pub picks: BTreeMap<UserId, Option<String>>,
    pub bans: SeriesPickBanBansSnapshot,
    pub patch_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuctionMatchGameRevertCommand {
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuctionTournamentCancelCommand {
    pub actor: BotActor,
    pub expected_revision: i64,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentCancelResult {
    pub tournament_id: AuctionTournamentId,
    pub tournament_status: AuctionTournamentStatus,
    pub tournament_revision: i64,
    pub source_recruitment_id: AuctionRecruitmentId,
    pub source_recruitment_revision: i64,
    pub reversed_game_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionRecruitmentForceCancelCommand {
    pub actor: BotActor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionRecruitmentForceCancelResult {
    pub recruitment_id: AuctionRecruitmentId,
    pub recruitment_status: RecruitmentStatus,
    pub recruitment_revision: i64,
    pub tournament_id: Option<AuctionTournamentId>,
    pub tournament_status: Option<AuctionTournamentStatus>,
    pub tournament_revision: Option<i64>,
    pub reversed_game_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentCancelPreviewTournament {
    pub id: AuctionTournamentId,
    pub display_number: i64,
    pub title: String,
    pub status: AuctionTournamentStatus,
    pub revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentCancelPreview {
    pub tournament: AuctionTournamentCancelPreviewTournament,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentResultTournamentSnapshot {
    pub id: AuctionTournamentId,
    pub title: String,
    pub display_number: i64,
    pub format: AuctionFormat,
    pub rules: AuctionRecruitmentSeriesRules,
    pub status: AuctionTournamentStatus,
    pub revision: i64,
    pub champion_team_id: Option<Uuid>,
    pub href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentResultTeamMemberSnapshot {
    pub team_member_id: Uuid,
    pub user_id: UserId,
    pub display_name: String,
    pub declared_lane: Option<Lane>,
    pub acquired_via: String,
    pub acquisition_points: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentResultTeamSnapshot {
    pub id: Uuid,
    pub team_index: i16,
    pub team_name: String,
    pub initial_points: i32,
    pub current_points: i32,
    pub spent_points: i32,
    pub members: Vec<AuctionTournamentResultTeamMemberSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentResultMatchSnapshot {
    pub id: AuctionMatchId,
    pub match_key: String,
    pub best_of: i16,
    pub hard_fearless: bool,
    pub status: String,
    pub left_team_id: Option<Uuid>,
    pub right_team_id: Option<Uuid>,
    pub left_score: i64,
    pub right_score: i64,
    pub winner_team_id: Option<Uuid>,
    pub lineups_locked: bool,
    pub lineups: Vec<AuctionMatchLineupAssignmentSnapshot>,
    pub games: Vec<AuctionMatchConfirmedGameSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentResultPlayerSnapshot {
    pub user_id: UserId,
    pub display_name: String,
    pub team_id: Uuid,
    pub games: i64,
    pub wins: i64,
    pub losses: i64,
    pub mmr_delta: Option<f64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentResultPermissions {
    pub can_revert_results: bool,
    pub can_recover_bracket: bool,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionTournamentResultSnapshot {
    pub tournament: AuctionTournamentResultTournamentSnapshot,
    pub teams: Vec<AuctionTournamentResultTeamSnapshot>,
    pub matches: Vec<AuctionTournamentResultMatchSnapshot>,
    pub players: Vec<AuctionTournamentResultPlayerSnapshot>,
    pub permissions: AuctionTournamentResultPermissions,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
enum WebAuctionMatchLineupMutation {
    Assign(AuctionMatchLineupAssignmentCommand),
    Lock(AuctionMatchLineupLockCommand),
    Unlock(AuctionMatchLineupLockCommand),
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
enum WebAuctionMatchGameMutation {
    Submit(AuctionMatchGameSubmitCommand),
    Revert(AuctionMatchGameRevertCommand),
}

#[derive(Serialize)]
struct WebAuctionMatchScopedIdempotencyCommand<'a, T: Serialize> {
    match_id: AuctionMatchId,
    command: &'a T,
}

#[derive(Serialize)]
struct BotAuctionTournamentCancelIdempotencyCommand<'a> {
    tournament_id: AuctionTournamentId,
    command: &'a AuctionTournamentCancelCommand,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebAuctionTournamentCancelCommand {
    expected_revision: i64,
    reason: String,
}

#[derive(Serialize)]
struct WebAuctionTournamentCancelIdempotencyCommand<'a> {
    tournament_id: AuctionTournamentId,
    command: &'a WebAuctionTournamentCancelCommand,
}

#[derive(Serialize)]
struct BotAuctionRecruitmentForceCancelIdempotencyCommand<'a> {
    recruitment_id: AuctionRecruitmentId,
    command: &'a AuctionRecruitmentForceCancelCommand,
}

impl WebAuctionMatchGameMutation {
    fn expected_revision(&self) -> i64 {
        match self {
            Self::Submit(command) => command.expected_revision,
            Self::Revert(command) => command.expected_revision,
        }
    }

    fn method(&self) -> &'static str {
        match self {
            Self::Submit(_) => "POST",
            Self::Revert(_) => "DELETE",
        }
    }

    fn route_template(&self) -> &'static str {
        match self {
            Self::Submit(_) => AUCTION_MATCH_GAME_SUBMIT_ROUTE,
            Self::Revert(_) => AUCTION_MATCH_GAME_REVERT_ROUTE,
        }
    }

    fn audit_action(&self) -> &'static str {
        match self {
            Self::Submit(_) => "AUCTION_MATCH_GAME_CONFIRMED",
            Self::Revert(_) => "AUCTION_MATCH_GAME_REVERTED",
        }
    }

    fn event_type(&self) -> &'static str {
        match self {
            Self::Submit(_) => "AUCTION_MATCH_GAME_CONFIRMED",
            Self::Revert(_) => "AUCTION_MATCH_GAME_REVERTED",
        }
    }
}

impl WebAuctionMatchLineupMutation {
    fn expected_revision(&self) -> i64 {
        match self {
            Self::Assign(command) => command.expected_revision,
            Self::Lock(command) => command.expected_revision,
            Self::Unlock(command) => command.expected_revision,
        }
    }

    fn audit_action(&self) -> &'static str {
        match self {
            Self::Assign(_) => "AUCTION_MATCH_LINEUP_ASSIGN",
            Self::Lock(_) => "AUCTION_MATCH_LINEUP_LOCK",
            Self::Unlock(_) => "AUCTION_MATCH_LINEUP_UNLOCK",
        }
    }

    fn method(&self) -> &'static str {
        match self {
            Self::Assign(_) => "PATCH",
            Self::Lock(_) => "POST",
            Self::Unlock(_) => "POST",
        }
    }

    fn route_template(&self) -> &'static str {
        match self {
            Self::Assign(_) => AUCTION_MATCH_LINEUP_ROUTE,
            Self::Lock(_) => AUCTION_MATCH_LINEUP_LOCK_ROUTE,
            Self::Unlock(_) => AUCTION_MATCH_LINEUP_UNLOCK_ROUTE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftSnapshot {
    pub recruitment: EntryDraftRecruitmentSnapshot,
    pub draft: EntryDraftStateSnapshot,
    #[serde(default)]
    pub head_to_head: Vec<EntryDraftHeadToHeadSnapshot>,
    pub capabilities: EntryDraftCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftRecruitmentSnapshot {
    pub id: RecruitmentId,
    pub display_number: i64,
    pub title: String,
    pub status: RecruitmentStatus,
    pub target_count: i16,
    pub best_of: i16,
    pub hard_fearless: bool,
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftStateSnapshot {
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub updated_by: Option<EntryDraftEditorSnapshot>,
    pub participants: Vec<EntryDraftParticipantSnapshot>,
    pub assignments: Vec<EntryDraftAssignmentSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftHeadToHeadSnapshot {
    pub lane: Lane,
    pub user_a_id: UserId,
    pub user_b_id: UserId,
    pub plays: i64,
    pub user_a_wins: i64,
    pub user_b_wins: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftEditorSnapshot {
    pub id: UserId,
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftParticipantSnapshot {
    pub id: UserId,
    pub display_name: String,
    pub preferred_lanes: Vec<Lane>,
    pub main_lane: Option<Lane>,
    pub mmr: f64,
    pub games: i32,
    pub wins: i32,
    pub riot_tier: Option<String>,
    pub profile_icon_id: Option<i32>,
    pub champion_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftAssignmentSnapshot {
    pub participant_id: UserId,
    pub team: String,
    pub lane: Lane,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftCapabilities {
    pub can_edit: bool,
    pub can_reset: bool,
    pub can_create_series: bool,
    pub can_create_auction_recruitment: bool,
    pub can_send_participant_confirmation_dms: bool,
    pub can_reopen: bool,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantConfirmationDmDispatchResult {
    pub queued_recipient_count: i64,
    pub already_queued_recipient_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesCreationResult {
    pub series: CreatedSeriesSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedSeriesSnapshot {
    pub id: SeriesId,
    pub href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryConfirmationSnapshot {
    series: EntryConfirmationSeriesSnapshot,
    teams: Vec<EntryConfirmationTeamSnapshot>,
    links: EntryConfirmationLinksSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryConfirmationSeriesSnapshot {
    id: SeriesId,
    display_number: i64,
    title: String,
    best_of: i16,
    hard_fearless: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryConfirmationTeamSnapshot {
    team: String,
    name: String,
    players: Vec<EntryConfirmationPlayerSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryConfirmationPlayerSnapshot {
    id: UserId,
    display_name: String,
    lane: Lane,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryConfirmationLinksSnapshot {
    pick_ban: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionMatchLineupConfirmationSnapshot {
    tournament: AuctionMatchLineupConfirmationTournamentSnapshot,
    #[serde(rename = "match")]
    matchup: AuctionMatchLineupConfirmationMatchSnapshot,
    teams: Vec<AuctionMatchLineupConfirmationTeamSnapshot>,
    links: AuctionMatchLineupConfirmationLinksSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionMatchLineupConfirmationTournamentSnapshot {
    id: AuctionTournamentId,
    display_number: i64,
    title: String,
    format: AuctionFormat,
    hard_fearless: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionMatchLineupConfirmationMatchSnapshot {
    id: AuctionMatchId,
    match_key: String,
    label: String,
    best_of: i16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionMatchLineupConfirmationTeamSnapshot {
    team_id: Uuid,
    name: String,
    players: Vec<AuctionMatchLineupConfirmationPlayerSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionMatchLineupConfirmationPlayerSnapshot {
    id: UserId,
    display_name: String,
    lane: Lane,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionMatchLineupConfirmationLinksSnapshot {
    pick_ban: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesResultNotificationSnapshot {
    series: SeriesResultNotificationSeriesSnapshot,
    teams: Vec<SeriesResultNotificationTeamSnapshot>,
    score: SeriesResultNotificationScoreSnapshot,
    games: Vec<SeriesResultNotificationGameSnapshot>,
    links: SeriesResultNotificationLinksSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionTournamentResultNotificationPayload {
    schema_version: u16,
    guild_id: GuildId,
    tournament_id: AuctionTournamentId,
    channel_id: DiscordChannelId,
    tournament: AuctionTournamentResultNotificationTournament,
    champion: AuctionTournamentResultNotificationChampion,
    matches: Vec<AuctionTournamentResultNotificationMatch>,
    #[serde(with = "time::serde::rfc3339")]
    completed_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionTournamentResultDeleteNotificationPayload {
    schema_version: u16,
    guild_id: GuildId,
    tournament_id: AuctionTournamentId,
    channel_id: DiscordChannelId,
    discord_message_id: DiscordMessageId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionTournamentResultNotificationTournament {
    display_number: i64,
    title: String,
    format: AuctionFormat,
    semifinal_best_of: Option<i16>,
    final_best_of: i16,
    hard_fearless: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionTournamentResultNotificationChampion {
    team_id: Uuid,
    team_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionTournamentResultNotificationMatch {
    match_key: String,
    label: String,
    best_of: i16,
    left_team_name: String,
    right_team_name: String,
    left_score: i64,
    right_score: i64,
    winner_team_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesResultNotificationSeriesSnapshot {
    id: SeriesId,
    display_number: i64,
    title: String,
    best_of: i16,
    hard_fearless: bool,
    winning_team: Option<String>,
    terminal_reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesResultNotificationScoreSnapshot {
    team_1: i64,
    team_2: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesResultNotificationTeamSnapshot {
    team: String,
    name: String,
    players: Vec<SeriesResultNotificationPlayerSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesResultNotificationPlayerSnapshot {
    id: UserId,
    display_name: String,
    lane: Lane,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesResultNotificationGameSnapshot {
    set_number: i16,
    winner: String,
    side_preset: String,
    team_1: SeriesResultNotificationGameTeamSnapshot,
    team_2: SeriesResultNotificationGameTeamSnapshot,
    bans: SeriesResultNotificationGameBansSnapshot,
    patch_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesResultNotificationGameBansSnapshot {
    blue: Vec<Option<String>>,
    red: Vec<Option<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesResultNotificationGameTeamSnapshot {
    team: String,
    name: String,
    picks: Vec<SeriesResultNotificationGamePickSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesResultNotificationGamePickSnapshot {
    id: UserId,
    display_name: String,
    lane: Lane,
    champion_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesResultNotificationLinksSnapshot {
    result: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanSnapshot {
    pub series: SeriesPickBanSeriesSnapshot,
    pub teams: Vec<SeriesPickBanTeamSnapshot>,
    pub current_game: SeriesPickBanGameSnapshot,
    pub games: Vec<SeriesPickBanGameSnapshot>,
    pub capabilities: SeriesPickBanCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanSeriesSnapshot {
    pub id: SeriesId,
    pub display_number: i64,
    pub title: String,
    pub best_of: i16,
    pub hard_fearless: bool,
    pub status: SeriesStatus,
    pub revision: i64,
    pub source_recruitment_id: RecruitmentId,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanTeamSnapshot {
    pub team: String,
    pub name: String,
    pub players: Vec<SeriesPickBanPlayerSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanPlayerSnapshot {
    pub id: UserId,
    pub display_name: String,
    pub lane: Lane,
    pub mmr: f64,
    pub riot_tier: Option<String>,
    pub preferred_champion_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanGameSnapshot {
    pub set_number: i16,
    pub side_preset: String,
    pub side_selected: bool,
    pub winner: Option<String>,
    pub picks: BTreeMap<String, Option<String>>,
    pub bans: SeriesPickBanBansSnapshot,
    pub patch_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct SeriesPickBanBansSnapshot {
    pub blue: Vec<Option<String>>,
    pub red: Vec<Option<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanCapabilities {
    pub can_edit: bool,
    pub can_submit: bool,
    pub can_revert_last_game: bool,
    pub can_early_complete: bool,
    pub can_revert: bool,
    pub can_delete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum SeriesPickBanOperation {
    SetSide {
        #[serde(rename = "sidePreset")]
        side_preset: String,
    },
    SetPick {
        #[serde(rename = "playerId")]
        player_id: UserId,
        #[serde(rename = "championId")]
        champion_id: String,
    },
    ClearPick {
        #[serde(rename = "playerId")]
        player_id: UserId,
    },
    SetBan {
        side: String,
        slot: i16,
        #[serde(rename = "championId")]
        champion_id: String,
    },
    ClearBan {
        side: String,
        slot: i16,
    },
    ApplyBulk {
        items: Vec<SeriesPickBanBulkItem>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum SeriesPickBanBulkItem {
    SetPick {
        #[serde(rename = "playerId")]
        player_id: UserId,
        #[serde(rename = "championId")]
        champion_id: String,
    },
    SetBan {
        side: String,
        slot: i16,
        #[serde(rename = "championId")]
        champion_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeriesGameSubmitCommand {
    pub set_number: i16,
    pub expected_revision: i64,
    pub side_preset: String,
    pub winner: String,
    pub picks: Option<BTreeMap<UserId, Option<String>>>,
    pub bans: Option<SeriesPickBanBansSnapshot>,
    pub patch_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeriesGameRevertCommand {
    pub expected_revision: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SeriesEarlyCompletionOutcome {
    #[serde(rename = "TEAM_1")]
    Team1,
    #[serde(rename = "TEAM_2")]
    Team2,
    #[serde(rename = "DRAW")]
    Draw,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeriesEarlyCompleteCommand {
    pub expected_revision: i64,
    pub outcome: SeriesEarlyCompletionOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesGameMutationResult {
    pub series: SeriesGameMutationSeries,
    pub confirmed_game_count: i64,
    pub next_href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesGameMutationSeries {
    pub id: SeriesId,
    pub status: SeriesStatus,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesResultSnapshot {
    pub series: SeriesResultSeriesSnapshot,
    pub teams: Vec<SeriesPickBanTeamSnapshot>,
    pub games: Vec<SeriesResultGameSnapshot>,
    pub score: SeriesResultScoreSnapshot,
    pub capabilities: SeriesResultCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesResultSeriesSnapshot {
    pub id: SeriesId,
    pub display_number: i64,
    pub title: String,
    pub best_of: i16,
    pub hard_fearless: bool,
    pub status: SeriesStatus,
    pub revision: i64,
    pub source_recruitment_id: RecruitmentId,
    pub winning_team: Option<String>,
    pub terminal_reason: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesResultGameSnapshot {
    pub id: GameId,
    pub set_number: i16,
    pub side_preset: String,
    pub winner: String,
    pub picks: BTreeMap<String, Option<String>>,
    pub bans: SeriesPickBanBansSnapshot,
    pub patch_version: Option<String>,
    pub duration_seconds: Option<i32>,
    pub mmr_changes: Vec<SeriesResultMmrChange>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesResultMmrChange {
    pub user_id: UserId,
    pub lane: Lane,
    pub delta: f64,
    pub mmr_before: f64,
    pub mmr_after: f64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesResultScoreSnapshot {
    pub team_1: i64,
    pub team_2: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesResultCapabilities {
    pub can_revert_last_game: bool,
    pub can_delete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesLifecycleResult {
    pub next_href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum EntryDraftOperation {
    MoveParticipant {
        #[serde(rename = "participantId")]
        participant_id: UserId,
        slot: Option<String>,
    },
}

pub async fn setup_guild(
    pool: &PgPool,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &RequestId,
    command: SetupGuildCommand,
) -> Result<SetupGuildResult, BotWorkflowError> {
    validate_setup_command(&command)?;

    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let idempotency_key = idempotency_key.clone();
    let request_id = request_id.0.clone();
    let prepared = SetupPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                setup_guild_attempt(
                    transaction,
                    &guild_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn register_guild_member(
    pool: &PgPool,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    command: RegisterGuildMemberCommand,
) -> Result<RegisterGuildMemberResult, BotWorkflowError> {
    validate_registration_input(&command.input)?;

    let request_hash = request_hash(guild_id, &command.input)?;
    let guild_id = guild_id.clone();
    let idempotency_key = idempotency_key.clone();
    let prepared = RegistrationPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let idempotency_key = idempotency_key.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                register_guild_member_attempt(
                    transaction,
                    &guild_id,
                    &idempotency_key,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn preflight_guild_registration(
    pool: &PgPool,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    input: &RegisterGuildMemberInput,
) -> Result<Option<RegisterGuildMemberResult>, BotWorkflowError> {
    validate_registration_input(input)?;
    let request_hash = request_hash(guild_id, input)?;
    let scope = TenantScope::guild(guild_id.clone());
    let outcome = idempotency::preflight(
        pool,
        &PreflightIdempotency {
            scope: &scope,
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
            method: REGISTRATION_METHOD,
            route_template: REGISTRATION_ROUTE,
            idempotency_key: idempotency_key.as_str(),
            request_hash: &request_hash,
            inspected_at: OffsetDateTime::now_utc(),
        },
    )
    .await
    .map_err(map_preflight_error)?;

    match outcome {
        PreflightIdempotencyOutcome::Missing => {
            let actor_user_id = UserId::from(&input.actor.id);
            require_registration_eligibility(pool, guild_id, &actor_user_id).await?;
            Ok(None)
        }
        PreflightIdempotencyOutcome::Replay { response } => {
            if response.status_code != 200 {
                return Err(BotWorkflowError::InvariantViolation);
            }
            let body = response.body.ok_or(BotWorkflowError::InvariantViolation)?;
            Ok(Some(serde_json::from_value(body)?))
        }
        PreflightIdempotencyOutcome::Conflict => Err(BotWorkflowError::IdempotencyConflict),
        PreflightIdempotencyOutcome::InProgress => Err(BotWorkflowError::IdempotencyInProgress),
    }
}

async fn require_registration_eligibility(
    pool: &PgPool,
    guild_id: &GuildId,
    user_id: &UserId,
) -> Result<(), BotWorkflowError> {
    let eligibility = sqlx::query_as::<_, (GuildStatus, bool, Option<GuildMemberStatus>)>(
        "SELECT guild.status,
                config.guild_id IS NOT NULL AS configured,
                member.status
         FROM guilds AS guild
         LEFT JOIN guild_config AS config ON config.guild_id = guild.guild_id
         LEFT JOIN guild_members AS member
           ON member.guild_id = guild.guild_id AND member.user_id = $2
         WHERE guild.guild_id = $1",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(BotWorkflowError::Database)?
    .ok_or(BotWorkflowError::GuildNotFound)?;
    if eligibility.0 != GuildStatus::Active {
        return Err(BotWorkflowError::GuildInactive(eligibility.0));
    }
    if !eligibility.1 {
        return Err(BotWorkflowError::GuildNotConfigured);
    }
    if eligibility.2 == Some(GuildMemberStatus::Inactive) {
        return Err(BotWorkflowError::MemberInactive);
    }
    Ok(())
}

pub async fn execute_recruitment_command(
    pool: &PgPool,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &RequestId,
    command: BotRecruitmentCommand,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    validate_recruitment_command(&command)?;

    if let BotRecruitmentCommand::Create {
        actor,
        rules,
        expected_start_at,
    } = &command
    {
        let request_hash = request_hash(guild_id, &command)?;
        return execute_recruitment_create(
            pool,
            guild_id,
            RecruitmentCreateActor::Bot(actor.clone()),
            *rules,
            *expected_start_at,
            IdempotencyPrincipalType::Bot,
            BOT_PRINCIPAL_ID,
            idempotency_key.as_str(),
            request_id,
            request_hash,
        )
        .await;
    }

    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let idempotency_key = idempotency_key.clone();
    let request_id = request_id.0.clone();
    let prepared = RecruitmentPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_recruitment_attempt(
                    transaction,
                    &guild_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn create_web_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    rules: RecruitmentSeriesRules,
    expected_start_at: OffsetDateTime,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    validate_recruitment_series_rules(&rules)?;
    let request_hash = request_hash(
        guild_id,
        &WebRecruitmentCreateRequest {
            rules,
            expected_start_at,
        },
    )?;
    execute_recruitment_create(
        pool,
        guild_id,
        RecruitmentCreateActor::Session(actor_id.clone()),
        rules,
        expected_start_at,
        IdempotencyPrincipalType::User,
        actor_id.as_str(),
        idempotency_key,
        request_id,
        request_hash,
    )
    .await
}

pub async fn execute_auction_recruitment_command(
    pool: &PgPool,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &RequestId,
    actor: BotActor,
    format: AuctionFormat,
    rules: AuctionRecruitmentSeriesRules,
    expected_start_at: OffsetDateTime,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    validate_actor(&actor)?;
    validate_auction_recruitment_series_rules(format, &rules)?;
    let command = AuctionRecruitmentCreateRequest {
        format,
        rules,
        expected_start_at,
    };
    let request_hash = request_hash(guild_id, &command)?;
    execute_auction_recruitment_create(
        pool,
        guild_id,
        AuctionRecruitmentCreateActor::Bot(actor),
        format,
        rules,
        expected_start_at,
        IdempotencyPrincipalType::Bot,
        BOT_PRINCIPAL_ID,
        idempotency_key.as_str(),
        request_id,
        request_hash,
    )
    .await
}

pub async fn execute_auction_recruitment_participation_command(
    pool: &PgPool,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &RequestId,
    command: BotAuctionRecruitmentParticipationCommand,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    validate_auction_recruitment_participation_command(&command)?;
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let idempotency_key = idempotency_key.clone();
    let request_id = request_id.0.clone();
    let prepared = AuctionRecruitmentMutationPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_auction_recruitment_participation_attempt(
                    transaction,
                    &guild_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn create_web_auction_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    format: AuctionFormat,
    rules: AuctionRecruitmentSeriesRules,
    expected_start_at: OffsetDateTime,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    validate_auction_recruitment_series_rules(format, &rules)?;
    let command = AuctionRecruitmentCreateRequest {
        format,
        rules,
        expected_start_at,
    };
    let request_hash = request_hash(guild_id, &command)?;
    execute_auction_recruitment_create(
        pool,
        guild_id,
        AuctionRecruitmentCreateActor::Session(actor_id.clone()),
        format,
        rules,
        expected_start_at,
        IdempotencyPrincipalType::User,
        actor_id.as_str(),
        idempotency_key,
        request_id,
        request_hash,
    )
    .await
}

pub async fn get_auction_recruitment_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    viewer_id: &UserId,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let snapshot =
        auction_recruitment_snapshot(&mut transaction, guild_id, recruitment_id, viewer_id)
            .await
            .map_err(AttemptError::into_public)?;
    if !matches!(
        snapshot.recruitment.status,
        RecruitmentStatus::Open | RecruitmentStatus::Closed
    ) {
        return Err(BotWorkflowError::RecruitmentNotFound);
    }
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

pub async fn get_auction_setup_draft_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    viewer_id: &UserId,
) -> Result<AuctionSetupDraftSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    require_active_session_member_read(&mut transaction, guild_id, viewer_id)
        .await
        .map_err(AttemptError::into_public)?;
    let snapshot =
        auction_setup_draft_snapshot(&mut transaction, guild_id, recruitment_id, viewer_id)
            .await
            .map_err(AttemptError::into_public)?;
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

pub async fn update_web_auction_setup_draft(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    actor_id: &UserId,
    expected_revision: i64,
    teams: Vec<AuctionSetupDraftTeamInput>,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionSetupDraftSnapshot, BotWorkflowError> {
    if expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected auction setup draft revision must be non-negative",
        ));
    }

    let command = WebAuctionSetupDraftUpdateCommand {
        recruitment_id,
        expected_revision,
        teams,
    };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebAuctionSetupDraftPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                execute_web_auction_setup_draft_update_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn create_web_auction_tournament(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    source_recruitment_id: AuctionRecruitmentId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionTournamentDraftSnapshot, BotWorkflowError> {
    if expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected auction setup draft revision must be non-negative",
        ));
    }

    let command = WebAuctionTournamentCreateCommand {
        source_recruitment_id,
        expected_revision,
    };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebAuctionTournamentCreatePrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                execute_web_auction_tournament_create_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn get_auction_tournament_draft_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    viewer_id: &UserId,
) -> Result<AuctionTournamentDraftSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let snapshot =
        auction_tournament_draft_snapshot(&mut transaction, guild_id, tournament_id, viewer_id)
            .await
            .map_err(AttemptError::into_public)?;
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

pub async fn execute_web_auction_tournament_draft_command(
    pool: &PgPool,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    command: AuctionTournamentDraftCommand,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionTournamentDraftSnapshot, BotWorkflowError> {
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebAuctionTournamentDraftPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                execute_web_auction_tournament_draft_command_attempt(
                    transaction,
                    &guild_id,
                    tournament_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn get_auction_tournament_bracket_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    viewer_id: &UserId,
) -> Result<AuctionTournamentBracketSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let snapshot =
        auction_tournament_bracket_snapshot(&mut transaction, guild_id, tournament_id, viewer_id)
            .await
            .map_err(AttemptError::into_public)?;
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

pub async fn execute_web_auction_tournament_bracket_command(
    pool: &PgPool,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    command: AuctionTournamentBracketCommand,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionTournamentBracketSnapshot, BotWorkflowError> {
    if command.expected_revision() < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected auction tournament bracket revision must be non-negative",
        ));
    }

    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebAuctionTournamentBracketPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                execute_web_auction_tournament_bracket_command_attempt(
                    transaction,
                    &guild_id,
                    tournament_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn cancel_auction_tournament(
    pool: &PgPool,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    idempotency_key: &DiscordInteractionId,
    request_id: &RequestId,
    command: AuctionTournamentCancelCommand,
) -> Result<AuctionTournamentCancelResult, BotWorkflowError> {
    validate_auction_tournament_cancel_command(&command)?;
    let request_hash = request_hash(
        guild_id,
        &BotAuctionTournamentCancelIdempotencyCommand {
            tournament_id,
            command: &command,
        },
    )?;
    let guild_id = guild_id.clone();
    let idempotency_key = idempotency_key.clone();
    let request_id = request_id.0.clone();
    let prepared = AuctionTournamentCancelPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                cancel_auction_tournament_attempt(
                    transaction,
                    &guild_id,
                    tournament_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn cancel_web_auction_tournament(
    pool: &PgPool,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    expected_revision: i64,
    reason: String,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionTournamentCancelResult, BotWorkflowError> {
    let command = WebAuctionTournamentCancelCommand {
        expected_revision,
        reason: reason.trim().to_owned(),
    };
    validate_web_auction_tournament_cancel_command(&command)?;
    let request_hash = request_hash(
        guild_id,
        &WebAuctionTournamentCancelIdempotencyCommand {
            tournament_id,
            command: &command,
        },
    )?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = AuctionTournamentCancelPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                cancel_web_auction_tournament_attempt(
                    transaction,
                    &guild_id,
                    tournament_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn force_cancel_auction_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    idempotency_key: &DiscordInteractionId,
    request_id: &RequestId,
    command: AuctionRecruitmentForceCancelCommand,
) -> Result<AuctionRecruitmentForceCancelResult, BotWorkflowError> {
    validate_actor(&command.actor)?;
    let request_hash = request_hash(
        guild_id,
        &BotAuctionRecruitmentForceCancelIdempotencyCommand {
            recruitment_id,
            command: &command,
        },
    )?;
    let guild_id = guild_id.clone();
    let idempotency_key = idempotency_key.clone();
    let request_id = request_id.0.clone();
    let prepared = AuctionRecruitmentForceCancelPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                force_cancel_auction_recruitment_attempt(
                    transaction,
                    &guild_id,
                    recruitment_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn get_auction_tournament_cancel_preview(
    pool: &PgPool,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<AuctionTournamentCancelPreview, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let preview =
        auction_tournament_cancel_preview(&mut transaction, guild_id, tournament_id).await?;
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;

    Ok(preview)
}

pub async fn get_web_auction_tournament_cancel_preview(
    pool: &PgPool,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
) -> Result<AuctionTournamentCancelPreview, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    if !is_active_session_operator(&mut transaction, guild_id, actor_id)
        .await
        .map_err(AttemptError::into_public)?
    {
        return Err(BotWorkflowError::OperatorRequired);
    }
    let preview =
        auction_tournament_cancel_preview(&mut transaction, guild_id, tournament_id).await?;
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;

    Ok(preview)
}

async fn auction_tournament_cancel_preview(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<AuctionTournamentCancelPreview, BotWorkflowError> {
    let row = sqlx::query_as::<_, AuctionTournamentCancelPreviewRow>(
        "SELECT tournament.id, tournament.display_number, tournament.title,
                tournament.status, tournament.revision
         FROM auction_tournaments AS tournament
         JOIN guilds AS guild
           ON guild.guild_id = tournament.guild_id
          AND guild.status = 'ACTIVE'
         WHERE tournament.guild_id = $1
           AND tournament.id = $2
           AND tournament.deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(BotWorkflowError::Database)?
    .ok_or(BotWorkflowError::AuctionTournamentNotFound)?;

    Ok(AuctionTournamentCancelPreview {
        tournament: AuctionTournamentCancelPreviewTournament {
            id: AuctionTournamentId::from_uuid(row.id),
            display_number: row.display_number,
            title: row.title,
            status: row.status,
            revision: row.revision,
        },
    })
}

pub async fn get_auction_match_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    viewer_id: &UserId,
) -> Result<AuctionMatchSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let snapshot = auction_match_snapshot(&mut transaction, guild_id, match_id, viewer_id)
        .await
        .map_err(AttemptError::into_public)?;
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

pub async fn get_auction_tournament_result_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    viewer_id: &UserId,
) -> Result<AuctionTournamentResultSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let snapshot =
        auction_tournament_result_snapshot(&mut transaction, guild_id, tournament_id, viewer_id)
            .await
            .map_err(AttemptError::into_public)?;
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

pub async fn update_web_auction_match_lineup(
    pool: &PgPool,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    command: AuctionMatchLineupAssignmentCommand,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionMatchSnapshot, BotWorkflowError> {
    execute_web_auction_match_lineup_mutation(
        pool,
        guild_id,
        match_id,
        actor_id,
        WebAuctionMatchLineupMutation::Assign(command),
        idempotency_key,
        request_id,
    )
    .await
}

pub async fn lock_web_auction_match_lineup(
    pool: &PgPool,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    command: AuctionMatchLineupLockCommand,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionMatchSnapshot, BotWorkflowError> {
    execute_web_auction_match_lineup_mutation(
        pool,
        guild_id,
        match_id,
        actor_id,
        WebAuctionMatchLineupMutation::Lock(command),
        idempotency_key,
        request_id,
    )
    .await
}

pub async fn unlock_web_auction_match_lineup(
    pool: &PgPool,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    command: AuctionMatchLineupLockCommand,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionMatchSnapshot, BotWorkflowError> {
    execute_web_auction_match_lineup_mutation(
        pool,
        guild_id,
        match_id,
        actor_id,
        WebAuctionMatchLineupMutation::Unlock(command),
        idempotency_key,
        request_id,
    )
    .await
}

pub async fn submit_web_auction_match_game(
    pool: &PgPool,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: AuctionMatchGameSubmitCommand,
) -> Result<AuctionMatchSnapshot, BotWorkflowError> {
    validate_auction_match_game_submit_command(&command)?;
    execute_web_auction_match_game_mutation(
        pool,
        guild_id,
        match_id,
        actor_id,
        WebAuctionMatchGameMutation::Submit(command),
        idempotency_key,
        request_id,
    )
    .await
}

pub async fn revert_last_web_auction_match_game(
    pool: &PgPool,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: AuctionMatchGameRevertCommand,
) -> Result<AuctionMatchSnapshot, BotWorkflowError> {
    if command.expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected auction match revision must be non-negative",
        ));
    }
    execute_web_auction_match_game_mutation(
        pool,
        guild_id,
        match_id,
        actor_id,
        WebAuctionMatchGameMutation::Revert(command),
        idempotency_key,
        request_id,
    )
    .await
}

async fn execute_web_auction_match_game_mutation(
    pool: &PgPool,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    command: WebAuctionMatchGameMutation,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionMatchSnapshot, BotWorkflowError> {
    let request_hash = request_hash(
        guild_id,
        &WebAuctionMatchScopedIdempotencyCommand {
            match_id,
            command: &command,
        },
    )?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebAuctionMatchGameMutationPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                execute_web_auction_match_game_mutation_attempt(
                    transaction,
                    &guild_id,
                    match_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

async fn execute_web_auction_match_lineup_mutation(
    pool: &PgPool,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    command: WebAuctionMatchLineupMutation,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionMatchSnapshot, BotWorkflowError> {
    if command.expected_revision() < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected auction match lineup revision must be non-negative",
        ));
    }

    let request_hash = request_hash(
        guild_id,
        &WebAuctionMatchScopedIdempotencyCommand {
            match_id,
            command: &command,
        },
    )?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebAuctionMatchLineupPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                execute_web_auction_match_lineup_mutation_attempt(
                    transaction,
                    &guild_id,
                    match_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn close_web_auction_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    execute_web_auction_recruitment_lifecycle(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebAuctionRecruitmentLifecycle::Close { recruitment_id },
    )
    .await
}

pub async fn dispatch_web_auction_recruitment_participant_confirmation_dms(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<ParticipantConfirmationDmDispatchResult, BotWorkflowError> {
    let command = WebAuctionRecruitmentParticipantConfirmationDmDispatchCommand { recruitment_id };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebParticipantConfirmationDmDispatchPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_auction_recruitment_participant_confirmation_dm_dispatch_attempt(
                    transaction,
                    &guild_id,
                    recruitment_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn cancel_web_auction_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    execute_web_auction_recruitment_lifecycle(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebAuctionRecruitmentLifecycle::Cancel { recruitment_id },
    )
    .await
}

pub async fn add_web_auction_recruitment_member(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    actor_id: &UserId,
    member_id: UserId,
    declared_lane: Lane,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    execute_web_auction_recruitment_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebAuctionRecruitmentMutation::AddMember {
            recruitment_id,
            member_id,
            declared_lane,
        },
    )
    .await
}

pub async fn remove_web_auction_recruitment_member(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    actor_id: &UserId,
    member_id: UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    execute_web_auction_recruitment_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebAuctionRecruitmentMutation::RemoveMember {
            recruitment_id,
            member_id,
        },
    )
    .await
}

pub async fn add_web_recruitment_member(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    member_id: UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    execute_web_recruitment_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebRecruitmentMutation::AddMember {
            recruitment_id,
            member_id,
        },
    )
    .await
}

pub async fn remove_web_recruitment_member(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    member_id: UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    execute_web_recruitment_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebRecruitmentMutation::RemoveMember {
            recruitment_id,
            member_id,
        },
    )
    .await
}

pub async fn close_web_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    execute_web_recruitment_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebRecruitmentMutation::Close { recruitment_id },
    )
    .await
}

pub async fn dispatch_web_recruitment_participant_confirmation_dms(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<ParticipantConfirmationDmDispatchResult, BotWorkflowError> {
    let command = WebRecruitmentParticipantConfirmationDmDispatchCommand { recruitment_id };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebParticipantConfirmationDmDispatchPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_recruitment_participant_confirmation_dm_dispatch_attempt(
                    transaction,
                    &guild_id,
                    recruitment_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn cancel_web_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    execute_web_recruitment_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebRecruitmentMutation::Cancel { recruitment_id },
    )
    .await
}

pub async fn update_web_entry_draft(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    expected_revision: i64,
    operation: EntryDraftOperation,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<EntryDraftSnapshot, BotWorkflowError> {
    execute_web_entry_draft_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebEntryDraftMutation::Apply {
            recruitment_id,
            expected_revision,
            operation,
        },
    )
    .await
}

pub async fn reset_web_entry_draft(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<EntryDraftSnapshot, BotWorkflowError> {
    execute_web_entry_draft_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebEntryDraftMutation::Reset {
            recruitment_id,
            expected_revision,
        },
    )
    .await
}

pub async fn reopen_web_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<EntryDraftSnapshot, BotWorkflowError> {
    execute_web_entry_draft_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebEntryDraftMutation::Reopen {
            recruitment_id,
            expected_revision,
        },
    )
    .await
}

pub async fn create_web_series(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    source_recruitment_id: RecruitmentId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<SeriesCreationResult, BotWorkflowError> {
    if expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected series source revision must be non-negative",
        ));
    }

    let command = WebSeriesCreateCommand {
        source_recruitment_id,
        expected_revision,
    };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebSeriesCreatePrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_series_create_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn convert_web_entry_draft_to_auction_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    source_recruitment_id: RecruitmentId,
    expected_revision: i64,
    expected_start_at: OffsetDateTime,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    if expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected source recruitment revision must be non-negative",
        ));
    }

    let command = WebEntryDraftAuctionConversionCommand {
        source_recruitment_id,
        expected_revision,
        expected_start_at,
    };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebEntryDraftAuctionConversionPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_entry_draft_auction_conversion_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn revert_web_series(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<SeriesLifecycleResult, BotWorkflowError> {
    execute_web_series_lifecycle(
        pool,
        guild_id,
        series_id,
        actor_id,
        expected_revision,
        idempotency_key,
        request_id,
        SeriesLifecycleAction::Revert,
    )
    .await
}

pub async fn delete_web_series(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<SeriesLifecycleResult, BotWorkflowError> {
    execute_web_series_lifecycle(
        pool,
        guild_id,
        series_id,
        actor_id,
        expected_revision,
        idempotency_key,
        request_id,
        SeriesLifecycleAction::Delete,
    )
    .await
}

pub async fn update_web_series_pick_ban(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    expected_revision: i64,
    operation: SeriesPickBanOperation,
    request_id: &RequestId,
) -> Result<SeriesPickBanSnapshot, BotWorkflowError> {
    validate_series_pick_ban_operation(&operation)?;
    if expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected series revision must be non-negative",
        ));
    }

    let command = WebSeriesPickBanCommand {
        series_id,
        expected_revision,
        operation,
    };
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let request_id = request_id.0.clone();
    let prepared = WebSeriesPickBanPrepared::new();
    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            Box::pin(async move {
                execute_web_series_pick_ban_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &request_id,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn submit_web_series_game(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: SeriesGameSubmitCommand,
) -> Result<SeriesGameMutationResult, BotWorkflowError> {
    validate_series_game_submit_command(&command)?;
    let command = WebSeriesGameSubmitCommand { series_id, command };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebSeriesGameMutationPrepared::new();
    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();
            Box::pin(async move {
                execute_web_series_game_submit_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn revert_last_web_series_game(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: SeriesGameRevertCommand,
) -> Result<SeriesGameMutationResult, BotWorkflowError> {
    if command.expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected series revision must be non-negative",
        ));
    }
    let command = WebSeriesGameRevertCommand { series_id, command };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebSeriesGameMutationPrepared::new();
    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();
            Box::pin(async move {
                execute_web_series_game_revert_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn complete_web_series_early(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: SeriesEarlyCompleteCommand,
) -> Result<SeriesGameMutationResult, BotWorkflowError> {
    if command.expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected series revision must be non-negative",
        ));
    }
    let command = WebSeriesEarlyCompleteCommand { series_id, command };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebSeriesEarlyCompletePrepared::new();
    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();
            Box::pin(async move {
                execute_web_series_early_complete_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn get_series_result_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    viewer_id: &UserId,
) -> Result<SeriesResultSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let snapshot = series_result_snapshot(&mut transaction, guild_id, series_id, viewer_id)
        .await
        .map_err(AttemptError::into_public)?;
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_series_lifecycle(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
    action: SeriesLifecycleAction,
) -> Result<SeriesLifecycleResult, BotWorkflowError> {
    if expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected series revision must be non-negative",
        ));
    }

    let command = WebSeriesLifecycleCommand {
        series_id,
        expected_revision,
        action,
    };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebSeriesLifecyclePrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_series_lifecycle_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn get_series_pick_ban_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    viewer_id: &UserId,
) -> Result<SeriesPickBanSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let snapshot = series_pick_ban_snapshot(&mut transaction, guild_id, series_id, viewer_id)
        .await
        .map_err(AttemptError::into_public)?;
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

pub async fn get_entry_draft_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    viewer_id: &UserId,
) -> Result<EntryDraftSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let snapshot = entry_draft_snapshot(&mut transaction, guild_id, recruitment_id, viewer_id)
        .await
        .map_err(AttemptError::into_public)?;
    if !matches!(
        snapshot.recruitment.status,
        RecruitmentStatus::Open | RecruitmentStatus::Closed
    ) {
        return Err(BotWorkflowError::RecruitmentNotFound);
    }
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

pub async fn get_recruitment_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    let snapshot = recruitment_snapshot(&mut transaction, guild_id, recruitment_id)
        .await
        .map_err(AttemptError::into_public)?;
    if !matches!(
        snapshot.recruitment.status,
        RecruitmentStatus::Open | RecruitmentStatus::Closed
    ) {
        return Err(BotWorkflowError::RecruitmentNotFound);
    }
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_recruitment_create(
    pool: &PgPool,
    guild_id: &GuildId,
    actor: RecruitmentCreateActor,
    rules: RecruitmentSeriesRules,
    expected_start_at: OffsetDateTime,
    principal_type: IdempotencyPrincipalType,
    principal_id: &str,
    idempotency_key: &str,
    request_id: &RequestId,
    request_hash: Vec<u8>,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    let guild_id = guild_id.clone();
    let principal_id = principal_id.to_owned();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = RecruitmentPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor = actor.clone();
            let rules = rules;
            let expected_start_at = expected_start_at;
            let principal_id = principal_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_recruitment_create_attempt(
                    transaction,
                    &guild_id,
                    &actor,
                    rules,
                    expected_start_at,
                    principal_type,
                    &principal_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

#[allow(clippy::too_many_arguments)]
async fn execute_auction_recruitment_create(
    pool: &PgPool,
    guild_id: &GuildId,
    actor: AuctionRecruitmentCreateActor,
    format: AuctionFormat,
    rules: AuctionRecruitmentSeriesRules,
    expected_start_at: OffsetDateTime,
    principal_type: IdempotencyPrincipalType,
    principal_id: &str,
    idempotency_key: &str,
    request_id: &RequestId,
    request_hash: Vec<u8>,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    let guild_id = guild_id.clone();
    let principal_id = principal_id.to_owned();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = AuctionRecruitmentPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor = actor.clone();
            let principal_id = principal_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_auction_recruitment_create_attempt(
                    transaction,
                    &guild_id,
                    &actor,
                    format,
                    rules,
                    expected_start_at,
                    principal_type,
                    &principal_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

async fn execute_web_auction_recruitment_lifecycle(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: WebAuctionRecruitmentLifecycle,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebAuctionRecruitmentLifecyclePrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                execute_web_auction_recruitment_lifecycle_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

async fn execute_web_auction_recruitment_mutation(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: WebAuctionRecruitmentMutation,
) -> Result<AuctionRecruitmentSnapshot, BotWorkflowError> {
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebAuctionRecruitmentMutationPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                execute_web_auction_recruitment_mutation_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

async fn execute_web_recruitment_mutation(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: WebRecruitmentMutation,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebRecruitmentMutationPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_recruitment_mutation_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

async fn execute_web_entry_draft_mutation(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: WebEntryDraftMutation,
) -> Result<EntryDraftSnapshot, BotWorkflowError> {
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebEntryDraftMutationPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_entry_draft_mutation_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

async fn setup_guild_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &str,
    request_hash: &[u8],
    command: &SetupGuildCommand,
    prepared: SetupPrepared,
) -> Result<SetupGuildResult, AttemptError> {
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<SetupGuildResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
            method: SETUP_METHOD,
            route_template: SETUP_ROUTE,
            key: idempotency_key.as_str(),
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let existing_status = lock_guild_for_setup(transaction, guild_id).await?;
    if let Some(status) = existing_status {
        require_guild_setup_available(transaction, guild_id, status).await?;
    }

    upsert_user(transaction, &command.actor, prepared.now).await?;
    if existing_status.is_some() {
        activate_guild(transaction, guild_id, command, prepared.now).await?;
    } else {
        let inserted = sqlx::query(
            "INSERT INTO guilds (
                 guild_id, discord_name, icon_hash, status, installed_by,
                 installed_at, activated_at, left_at, updated_at
             ) VALUES ($1, $2, $3, 'ACTIVE', $4, $5, $5, NULL, $5)
             ON CONFLICT (guild_id) DO NOTHING",
        )
        .bind(guild_id)
        .bind(command.guild_name.trim())
        .bind(command.guild_icon_hash.as_deref())
        .bind(&command.actor.id)
        .bind(prepared.now)
        .execute(&mut **transaction)
        .await?;

        if inserted.rows_affected() == 0 {
            let status = lock_guild_for_setup(transaction, guild_id)
                .await?
                .ok_or(AttemptError::GuildNotFound)?;
            require_guild_setup_available(transaction, guild_id, status).await?;
            activate_guild(transaction, guild_id, command, prepared.now).await?;
        }
    }

    sync_guild_actor(transaction, guild_id, &command.actor, prepared.now).await?;

    let config_revision = sqlx::query_scalar::<_, i64>(
        "INSERT INTO guild_config (
             guild_id, display_name, operator_role_id, recruitment_channel_id,
             entry_channel_id, result_channel_id, log_channel_id, is_externally_public, revision, updated_by, updated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, false, 0, $8, $9)
         RETURNING revision",
    )
    .bind(guild_id)
    .bind(command.guild_name.trim())
    .bind(&command.operator_role_id)
    .bind(&command.recruitment_channel_id)
    .bind(&command.entry_channel_id)
    .bind(&command.result_channel_id)
    .bind(&command.log_channel_id)
    .bind(&command.actor.id)
    .bind(prepared.now)
    .fetch_one(&mut **transaction)
    .await?;

    let current_season = match sqlx::query_as::<_, SeasonRow>(
        "SELECT id, name
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    {
        Some(row) => row.into_snapshot(),
        None => {
            sqlx::query(
                "INSERT INTO seasons (
                     guild_id, id, name, status, started_at, ended_at, created_by, created_at
                 ) VALUES ($1, $2, $3, 'ACTIVE', $4, NULL, $5, $4)",
            )
            .bind(guild_id)
            .bind(prepared.season_id.as_uuid())
            .bind(command.season_name.trim())
            .bind(prepared.now)
            .bind(&command.actor.id)
            .execute(&mut **transaction)
            .await?;

            BotSeasonSnapshot {
                id: prepared.season_id,
                name: command.season_name.trim().to_owned(),
            }
        }
    };

    for counter_key in [
        "RECRUITMENT",
        "SERIES",
        "AUCTION_RECRUITMENT",
        "AUCTION_TOURNAMENT",
    ] {
        sqlx::query(
            "INSERT INTO guild_counters (guild_id, counter_key, next_value)
             VALUES ($1, $2, 1)
             ON CONFLICT (guild_id, counter_key) DO NOTHING",
        )
        .bind(guild_id)
        .bind(counter_key)
        .execute(&mut **transaction)
        .await?;
    }

    let result = SetupGuildResult {
        guild_id: guild_id.clone(),
        display_name: command.guild_name.trim().to_owned(),
        operator_role_id: command.operator_role_id.clone(),
        current_season,
        config_revision,
        dashboard_href: format!("/g/{guild_id}"),
    };

    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: command.actor.id.as_str(),
            action: "GUILD_SETUP",
            target_type: "GUILD",
            target_id: guild_id.as_str(),
            request_id,
            before_summary: None,
            after_summary: Some(serde_json::to_value(&result)?),
            now: prepared.now,
        },
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;

    Ok(result)
}

async fn lock_guild_for_setup(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<Option<GuildStatus>, AttemptError> {
    Ok(sqlx::query_scalar::<_, GuildStatus>(
        "SELECT status FROM guilds WHERE guild_id = $1 FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn require_guild_setup_available(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    status: GuildStatus,
) -> Result<(), AttemptError> {
    let has_config = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM guild_config WHERE guild_id = $1)",
    )
    .bind(guild_id)
    .fetch_one(&mut **transaction)
    .await?;

    if status == GuildStatus::Active || has_config {
        return Err(AttemptError::GuildAlreadyConfigured);
    }
    match status {
        GuildStatus::Installed | GuildStatus::SetupRequired => Ok(()),
        GuildStatus::Disabled | GuildStatus::Left => {
            Err(AttemptError::GuildSetupUnavailable(status))
        }
        GuildStatus::Active => unreachable!("ACTIVE guilds are rejected above"),
    }
}

async fn activate_guild(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    command: &SetupGuildCommand,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "UPDATE guilds
         SET discord_name = $2,
             icon_hash = $3,
             status = 'ACTIVE',
             installed_by = $4,
             activated_at = COALESCE(activated_at, $5),
             left_at = NULL,
             updated_at = $5
         WHERE guild_id = $1",
    )
    .bind(guild_id)
    .bind(command.guild_name.trim())
    .bind(command.guild_icon_hash.as_deref())
    .bind(&command.actor.id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn register_guild_member_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_hash: &[u8],
    command: &RegisterGuildMemberCommand,
    prepared: RegistrationPrepared,
) -> Result<RegisterGuildMemberResult, AttemptError> {
    let _operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<RegisterGuildMemberResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
            method: REGISTRATION_METHOD,
            route_template: REGISTRATION_ROUTE,
            key: idempotency_key.as_str(),
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let existing_status = sqlx::query_scalar::<_, GuildMemberStatus>(
        "SELECT status
         FROM guild_members
         WHERE guild_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(&command.input.actor.id)
    .fetch_optional(&mut **transaction)
    .await?;
    if existing_status == Some(GuildMemberStatus::Inactive) {
        return Err(AttemptError::MemberInactive);
    }
    let registration = if existing_status.is_some() {
        RegistrationState::Existing
    } else {
        RegistrationState::Created
    };

    sync_guild_actor(transaction, guild_id, &command.input.actor, prepared.now).await?;
    let actor_user_id = UserId::from(&command.input.actor.id);
    let linked =
        riot::link_account_in_transaction(transaction, &actor_user_id, &command.account).await?;
    let account = linked
        .accounts
        .into_iter()
        .find(|account| account.puuid == command.account.puuid)
        .ok_or(AttemptError::InvariantViolation)?;
    let result = RegisterGuildMemberResult {
        member: RegisteredGuildMemberSnapshot {
            id: command.input.actor.id.clone(),
            display_name: command.input.actor.display_name.clone(),
            status: GuildMemberStatus::Active,
        },
        riot_account: registered_riot_account(account),
        registration,
        account_link: match linked.state {
            RiotAccountLinkState::Created => RegistrationState::Created,
            RiotAccountLinkState::Existing => RegistrationState::Existing,
        },
    };

    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;
    Ok(result)
}

async fn execute_recruitment_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &str,
    request_hash: &[u8],
    command: &BotRecruitmentCommand,
    prepared: RecruitmentPrepared,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let metadata = command.metadata();
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<RecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key.as_str(),
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let actor = command.actor();
    let actor_user_id = UserId::from(&actor.id);
    sync_guild_actor(transaction, guild_id, actor, prepared.now).await?;
    require_active_member(transaction, guild_id, &actor.id).await?;

    let (before, snapshot) = match command {
        // Create commands are dispatched to the shared Bot/session create transaction before
        // this mutation-only command path is entered.
        BotRecruitmentCommand::Create { .. } => return Err(AttemptError::InvariantViolation),
        BotRecruitmentCommand::BindMessage {
            recruitment_id,
            channel_id,
            message_id,
            ..
        } => {
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            require_creator_or_operator(actor, &locked.created_by, &operator_role_id)?;
            ensure_bindable(locked.status)?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            sqlx::query(
                "UPDATE recruitments
                 SET channel_id = $1,
                     message_id = $2,
                     revision = revision + 1,
                     updated_by = $3,
                     updated_at = $4
                 WHERE guild_id = $5 AND id = $6",
            )
            .bind(channel_id)
            .bind(message_id)
            .bind(&actor.id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        BotRecruitmentCommand::Join {
            recruitment_id,
            participant,
            ..
        } => {
            let participant_user_id = UserId::from(&participant.id);
            require_self_or_operator(actor, &participant_user_id, &operator_role_id)?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            lock_entry_draft(transaction, guild_id, *recruitment_id).await?;
            if actor.id != participant.id {
                sync_guild_actor(transaction, guild_id, participant, prepared.now).await?;
                require_active_member(transaction, guild_id, &participant.id).await?;
            }
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            join_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                &participant_user_id,
                locked.target_count,
                prepared.now,
            )
            .await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                &actor_user_id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        BotRecruitmentCommand::Leave {
            recruitment_id,
            user_id,
            ..
        } => {
            require_self_or_operator(actor, user_id, &operator_role_id)?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            lock_entry_draft(transaction, guild_id, *recruitment_id).await?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            leave_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                user_id,
                prepared.now,
            )
            .await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                &actor_user_id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        BotRecruitmentCommand::SetPreferredLanes {
            recruitment_id,
            user_id,
            lanes,
            ..
        } => {
            require_self_or_operator(actor, user_id, &operator_role_id)?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            lock_entry_draft(transaction, guild_id, *recruitment_id).await?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            set_preferred_lanes(transaction, guild_id, *recruitment_id, user_id, lanes).await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                &actor_user_id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        BotRecruitmentCommand::Close { recruitment_id, .. } => {
            require_operator(actor, &operator_role_id)?;
            let (before, snapshot) = close_recruitment_for_entry(
                transaction,
                guild_id,
                *recruitment_id,
                &actor_user_id,
                prepared.now,
            )
            .await?;
            (Some(before), snapshot)
        }
    };

    let recruitment_id = snapshot.recruitment.id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor.id.as_str(),
            action: metadata.audit_action,
            target_type: "RECRUITMENT",
            target_id: &recruitment_id,
            request_id,
            before_summary: before.as_ref().map(serde_json::to_value).transpose()?,
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:recruitment:{}", snapshot.recruitment.id),
        "RECRUITMENT",
        &recruitment_id,
        snapshot.recruitment.revision,
        metadata.event_type,
        json!({
            "type": "invalidate",
            "resourceId": snapshot.recruitment.id,
            "revision": snapshot.recruitment.revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(
        transaction,
        &scope,
        record_id,
        metadata.success_status,
        &snapshot,
        prepared.now,
    )
    .await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_recruitment_mutation_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebRecruitmentMutation,
    prepared: WebRecruitmentMutationPrepared,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let metadata = command.metadata();
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<RecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let (before, snapshot) = match command {
        WebRecruitmentMutation::AddMember {
            recruitment_id,
            member_id,
        } => {
            require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id)
                .await?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            lock_entry_draft(transaction, guild_id, *recruitment_id).await?;
            require_active_recruitment_member(transaction, guild_id, member_id).await?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            join_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                member_id,
                locked.target_count,
                prepared.now,
            )
            .await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        WebRecruitmentMutation::RemoveMember {
            recruitment_id,
            member_id,
        } => {
            require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id)
                .await?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            lock_entry_draft(transaction, guild_id, *recruitment_id).await?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            leave_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                member_id,
                prepared.now,
            )
            .await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        WebRecruitmentMutation::Close { recruitment_id } => {
            require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id)
                .await?;
            let (before, snapshot) = close_recruitment_for_entry(
                transaction,
                guild_id,
                *recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
            (Some(before), snapshot)
        }
        WebRecruitmentMutation::Cancel { recruitment_id } => {
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            require_active_creator_or_operator(
                transaction,
                guild_id,
                actor_id,
                &locked.created_by,
                &operator_role_id,
            )
            .await?;
            ensure_cancellable(locked.status)?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            let cancelled = sqlx::query(
                "UPDATE recruitments
                 SET status = 'CANCELLED',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     cancelled_at = $2
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            if cancelled.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
    };

    let recruitment_id = snapshot.recruitment.id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: metadata.audit_action,
            target_type: "RECRUITMENT",
            target_id: &recruitment_id,
            request_id,
            before_summary: before.as_ref().map(serde_json::to_value).transpose()?,
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:recruitment:{}", snapshot.recruitment.id),
        "RECRUITMENT",
        &recruitment_id,
        snapshot.recruitment.revision,
        metadata.event_type,
        json!({
            "type": "invalidate",
            "resourceId": snapshot.recruitment.id,
            "revision": snapshot.recruitment.revision,
        }),
        prepared.now,
    )
    .await?;
    enqueue_web_recruitment_message_notification(
        transaction,
        guild_id,
        &snapshot,
        command,
        prepared.notification_id,
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_recruitment_participant_confirmation_dm_dispatch_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    prepared: WebParticipantConfirmationDmDispatchPrepared,
) -> Result<ParticipantConfirmationDmDispatchResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<ParticipantConfirmationDmDispatchResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: "POST",
            route_template: RECRUITMENT_PARTICIPANT_CONFIRMATION_DMS_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let locked = lock_recruitment(transaction, guild_id, recruitment_id).await?;
    ensure_participant_confirmation_dms_dispatchable(locked.status)?;
    let snapshot = recruitment_snapshot(transaction, guild_id, recruitment_id).await?;
    let result = enqueue_recruitment_participant_confirmation_direct_messages(
        transaction,
        guild_id,
        &snapshot,
        prepared.now,
    )
    .await?;
    let target_id = recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "RECRUITMENT_PARTICIPANT_CONFIRMATION_DMS_REQUESTED",
            target_type: "RECRUITMENT",
            target_id: &target_id,
            request_id,
            before_summary: None,
            after_summary: Some(serde_json::to_value(&result)?),
            now: prepared.now,
        },
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_entry_draft_mutation_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebEntryDraftMutation,
    prepared: WebEntryDraftMutationPrepared,
) -> Result<EntryDraftSnapshot, AttemptError> {
    let metadata = command.metadata();
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;

    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<EntryDraftSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let recruitment_id = command.recruitment_id();
    let locked = lock_recruitment(transaction, guild_id, recruitment_id).await?;
    let draft_revision = lock_entry_draft_revision(transaction, guild_id, recruitment_id).await?;
    let actual_revision = match command {
        WebEntryDraftMutation::Reopen { .. } => locked.revision,
        WebEntryDraftMutation::Apply { .. } | WebEntryDraftMutation::Reset { .. } => draft_revision,
    };
    if actual_revision != command.expected_revision() {
        return Err(AttemptError::EntryDraftRevisionConflict {
            expected: command.expected_revision(),
            actual: actual_revision,
        });
    }

    let before = entry_draft_snapshot(transaction, guild_id, recruitment_id, actor_id).await?;
    let mut message_snapshot = None;
    match command {
        WebEntryDraftMutation::Apply { operation, .. } => {
            ensure_entry_editable(locked.status)?;
            apply_entry_draft_operation(transaction, guild_id, recruitment_id, operation).await?;
            bump_entry_draft(
                transaction,
                guild_id,
                recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
        }
        WebEntryDraftMutation::Reset { .. } => {
            ensure_entry_editable(locked.status)?;
            sqlx::query(
                "DELETE FROM recruitment_entry_assignments
                 WHERE guild_id = $1 AND recruitment_id = $2",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            bump_entry_draft(
                transaction,
                guild_id,
                recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
        }
        WebEntryDraftMutation::Reopen { .. } => {
            ensure_reopenable(locked.status)?;
            let reopened = sqlx::query(
                "UPDATE recruitments
                 SET status = 'OPEN',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     closed_at = NULL,
                     cancelled_at = NULL
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            if reopened.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
            bump_entry_draft(
                transaction,
                guild_id,
                recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
            message_snapshot =
                Some(recruitment_snapshot(transaction, guild_id, recruitment_id).await?);
        }
    }

    let snapshot = entry_draft_snapshot(transaction, guild_id, recruitment_id, actor_id).await?;
    let target_id = recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: metadata.audit_action,
            target_type: "RECRUITMENT_ENTRY_DRAFT",
            target_id: &target_id,
            request_id,
            before_summary: Some(serde_json::to_value(&before)?),
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:recruitment-entry-draft:{recruitment_id}"),
        "RECRUITMENT_ENTRY_DRAFT",
        &target_id,
        snapshot.draft.revision,
        metadata.event_type,
        json!({
            "type": "invalidate",
            "resourceId": recruitment_id,
            "revision": snapshot.draft.revision,
        }),
        prepared.now,
    )
    .await?;
    if matches!(command, WebEntryDraftMutation::Reopen { .. }) {
        insert_outbox(
            transaction,
            guild_id,
            prepared.recruitment_outbox_event_id,
            format!("guild:{guild_id}:recruitment:{recruitment_id}"),
            "RECRUITMENT",
            &target_id,
            snapshot.recruitment.revision,
            metadata.event_type,
            json!({
                "type": "invalidate",
                "resourceId": recruitment_id,
                "revision": snapshot.recruitment.revision,
            }),
            prepared.now,
        )
        .await?;
    }
    if let Some(message_snapshot) = message_snapshot.as_ref() {
        enqueue_recruitment_message_update(
            transaction,
            guild_id,
            message_snapshot,
            prepared.notification_id,
            prepared.now,
        )
        .await?;
    }
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_series_create_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebSeriesCreateCommand,
    prepared: WebSeriesCreatePrepared,
) -> Result<SeriesCreationResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;

    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<SeriesCreationResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: SERIES_CREATE_METHOD,
            route_template: SERIES_CREATE_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };
    let entry_channel_id = configured_entry_channel(transaction, guild_id).await?;

    // Preserve the common recruitment -> entry-draft lock order used by every entry mutation.
    let recruitment =
        lock_recruitment(transaction, guild_id, command.source_recruitment_id).await?;
    if recruitment.revision != command.expected_revision {
        return Err(AttemptError::RecruitmentRevisionConflict {
            expected: command.expected_revision,
            actual: recruitment.revision,
        });
    }
    ensure_series_source_ready(recruitment.status)?;
    lock_entry_draft(transaction, guild_id, command.source_recruitment_id).await?;

    let assignments = locked_full_entry_assignments(
        transaction,
        guild_id,
        command.source_recruitment_id,
        recruitment.target_count,
    )
    .await?;
    let mmr_rows = mmr::ensure_and_lock_mmr_rows(
        transaction,
        guild_id,
        SeasonId::from_uuid(recruitment.season_id),
        assignments.iter().map(|assignment| MmrLaneKey {
            user_id: assignment.user_id.clone(),
            lane: assignment.lane,
        }),
        prepared.now,
    )
    .await?;
    let mmr_by_assignment = mmr_rows
        .into_iter()
        .map(|row| ((row.key.user_id, row.key.lane), row.mmr))
        .collect::<HashMap<_, _>>();
    if mmr_by_assignment.len() != assignments.len() {
        return Err(AttemptError::InvariantViolation);
    }

    let display_number = next_series_display_number(transaction, guild_id).await?;
    let title = format!("일반 내전 #{display_number}");
    sqlx::query(
        "INSERT INTO series (
             guild_id, id, season_id, source_recruitment_id, display_number, title,
             best_of, hard_fearless, status, winning_team, terminal_reason, revision,
             started_at, ended_at, created_by, end_card_channel_id, end_card_message_id,
             updated_by, updated_at, deleted_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6,
             $7, $8, 'IN_PROGRESS', NULL, NULL, 0,
             $9, NULL, $10, NULL, NULL,
             $10, $9, NULL
         )",
    )
    .bind(guild_id)
    .bind(prepared.series_id.as_uuid())
    .bind(recruitment.season_id)
    .bind(command.source_recruitment_id.as_uuid())
    .bind(display_number)
    .bind(&title)
    .bind(recruitment.best_of)
    .bind(recruitment.hard_fearless)
    .bind(prepared.now)
    .bind(actor_id)
    .execute(&mut **transaction)
    .await?;

    for assignment in &assignments {
        let mmr_at_lock = mmr_by_assignment
            .get(&(assignment.user_id.clone(), assignment.lane))
            .copied()
            .ok_or(AttemptError::InvariantViolation)?;
        sqlx::query(
            "INSERT INTO series_participants (
                 guild_id, series_id, user_id, team, lane, mmr_at_lock, mmr_snapshot_origin
             ) VALUES ($1, $2, $3, $4, $5, $6, 'CAPTURED')",
        )
        .bind(guild_id)
        .bind(prepared.series_id.as_uuid())
        .bind(&assignment.user_id)
        .bind(&assignment.team)
        .bind(assignment.lane)
        .bind(mmr_at_lock)
        .execute(&mut **transaction)
        .await?;
    }

    let converted_revision = recruitment
        .revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;
    let converted = sqlx::query(
        "UPDATE recruitments
         SET status = 'CONVERTED',
             revision = revision + 1,
             updated_by = $1,
             updated_at = $2,
             converted_at = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(actor_id)
    .bind(prepared.now)
    .bind(guild_id)
    .bind(command.source_recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if converted.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    let converted_recruitment_snapshot =
        recruitment_snapshot(transaction, guild_id, command.source_recruitment_id).await?;
    enqueue_recruitment_message_update(
        transaction,
        guild_id,
        &converted_recruitment_snapshot,
        prepared.recruitment_message_notification_id,
        prepared.now,
    )
    .await?;
    if let Some(channel_id) = entry_channel_id.as_ref() {
        let snapshot = entry_confirmation_snapshot(
            transaction,
            guild_id,
            prepared.series_id,
            display_number,
            &title,
            recruitment.best_of,
            recruitment.hard_fearless,
        )
        .await?;
        enqueue_entry_confirmation_notification(
            transaction,
            guild_id,
            channel_id,
            &snapshot,
            prepared.entry_confirmation_notification_id,
            prepared.now,
        )
        .await?;
    }

    let result = SeriesCreationResult {
        series: CreatedSeriesSnapshot {
            id: prepared.series_id,
            href: format!("/g/{guild_id}/series/{}/pick-ban", prepared.series_id),
        },
    };
    let source_id = command.source_recruitment_id.to_string();
    let series_id = prepared.series_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "SERIES_CREATED",
            target_type: "SERIES",
            target_id: &series_id,
            request_id,
            before_summary: Some(json!({
                "sourceRecruitmentId": command.source_recruitment_id,
                "sourceRecruitmentRevision": recruitment.revision,
                "assignmentCount": assignments.len(),
            })),
            after_summary: Some(serde_json::to_value(&result)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.recruitment_outbox_event_id,
        format!(
            "guild:{guild_id}:recruitment:{}",
            command.source_recruitment_id
        ),
        "RECRUITMENT",
        &source_id,
        converted_revision,
        "RECRUITMENT_CONVERTED",
        json!({
            "type": "invalidate",
            "resourceId": command.source_recruitment_id,
            "revision": converted_revision,
        }),
        prepared.now,
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.series_outbox_event_id,
        format!("guild:{guild_id}:series:{}", prepared.series_id),
        "SERIES",
        &series_id,
        0,
        "SERIES_CREATED",
        json!({
            "type": "invalidate",
            "resourceId": prepared.series_id,
            "revision": 0,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 201, &result, prepared.now).await?;

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_entry_draft_auction_conversion_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebEntryDraftAuctionConversionCommand,
    prepared: WebEntryDraftAuctionConversionPrepared,
) -> Result<AuctionRecruitmentSnapshot, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;

    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionRecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: "POST",
            route_template: ENTRY_DRAFT_AUCTION_CONVERT_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };
    let channel_id = require_recruitment_channel(transaction, guild_id).await?;

    // Keep the regular side in the same recruitment -> entry-draft order as entry mutations.
    let recruitment =
        lock_recruitment(transaction, guild_id, command.source_recruitment_id).await?;
    if recruitment.revision != command.expected_revision {
        return Err(AttemptError::RecruitmentRevisionConflict {
            expected: command.expected_revision,
            actual: recruitment.revision,
        });
    }
    ensure_entry_draft_auction_source_ready(recruitment.status)?;
    lock_entry_draft(transaction, guild_id, command.source_recruitment_id).await?;
    if active_series_exists_for_source_recruitment(
        transaction,
        guild_id,
        command.source_recruitment_id,
    )
    .await?
    {
        return Err(AttemptError::EntryDraftAlreadyConvertedToSeries);
    }

    let participants = locked_entry_draft_auction_conversion_participants(
        transaction,
        guild_id,
        command.source_recruitment_id,
        recruitment.target_count,
        SeasonId::from_uuid(recruitment.season_id),
        prepared.now,
    )
    .await?;

    let format = AuctionFormat::TenPerson;
    let display_number = next_auction_recruitment_display_number(transaction, guild_id).await?;
    let title = auction_recruitment_title(format, display_number, command.expected_start_at)?;
    sqlx::query(
        "INSERT INTO auction_recruitments (
             guild_id, id, season_id, display_number, title, format, target_count,
             semifinal_best_of, final_best_of, hard_fearless, status, revision, channel_id,
             message_id, created_by, created_at, updated_by, updated_at, closed_at,
             converted_at, cancelled_at
         ) VALUES (
             $1, $2, $3, $4, $5, 'TEN_PERSON', 10,
             NULL, $6, $7, 'CLOSED', 1, NULL,
             NULL, $8, $9, $8, $9, $9,
             NULL, NULL
         )",
    )
    .bind(guild_id)
    .bind(prepared.auction_recruitment_id.as_uuid())
    .bind(recruitment.season_id)
    .bind(display_number)
    .bind(&title)
    .bind(recruitment.best_of)
    .bind(recruitment.hard_fearless)
    .bind(actor_id)
    .bind(prepared.now)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO auction_setup_drafts (
             guild_id, recruitment_id, revision, updated_by, updated_at
         ) VALUES ($1, $2, 0, $3, $4)",
    )
    .bind(guild_id)
    .bind(prepared.auction_recruitment_id.as_uuid())
    .bind(actor_id)
    .bind(prepared.now)
    .execute(&mut **transaction)
    .await?;
    for team_index in 1..=format.team_count() {
        sqlx::query(
            "INSERT INTO auction_setup_teams (
                 guild_id, recruitment_id, team_index, team_name, captain_user_id, initial_points
             ) VALUES ($1, $2, $3, $4, NULL, $5)",
        )
        .bind(guild_id)
        .bind(prepared.auction_recruitment_id.as_uuid())
        .bind(team_index)
        .bind(format!("팀 {team_index}"))
        .bind(AUCTION_INITIAL_TEAM_POINTS)
        .execute(&mut **transaction)
        .await?;
    }

    for (index, participant) in participants.iter().enumerate() {
        let offset = Duration::milliseconds(
            i64::try_from(index).map_err(|_| AttemptError::InvariantViolation)?,
        );
        sqlx::query(
            "INSERT INTO auction_recruitment_participants (
                 guild_id, recruitment_id, user_id, status, declared_lane, mmr_at_join,
                 snapshot_origin, joined_at, withdrawn_at
             ) VALUES ($1, $2, $3, 'JOINED', $4, $5, $6, $7, NULL)",
        )
        .bind(guild_id)
        .bind(prepared.auction_recruitment_id.as_uuid())
        .bind(&participant.user_id)
        .bind(participant.declared_lane)
        .bind(participant.mmr_at_join)
        .bind(participant.snapshot_origin)
        .bind(prepared.now + offset)
        .execute(&mut **transaction)
        .await?;
    }

    let converted_revision = recruitment
        .revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;
    let converted = sqlx::query(
        "UPDATE recruitments
         SET status = 'CONVERTED',
             revision = revision + 1,
             updated_by = $1,
             updated_at = $2,
             converted_at = $2
         WHERE guild_id = $3 AND id = $4 AND status = 'CLOSED'",
    )
    .bind(actor_id)
    .bind(prepared.now)
    .bind(guild_id)
    .bind(command.source_recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if converted.rows_affected() != 1 {
        return Err(AttemptError::EntryDraftAuctionSourceNotReady(
            recruitment.status,
        ));
    }

    let source_snapshot =
        recruitment_snapshot(transaction, guild_id, command.source_recruitment_id).await?;
    enqueue_recruitment_message_update(
        transaction,
        guild_id,
        &source_snapshot,
        prepared.regular_recruitment_notification_id,
        prepared.now,
    )
    .await?;

    let snapshot = auction_recruitment_snapshot(
        transaction,
        guild_id,
        prepared.auction_recruitment_id,
        actor_id,
    )
    .await?;
    enqueue_auction_recruitment_message_notification(
        transaction,
        guild_id,
        &channel_id,
        &snapshot,
        AUCTION_RECRUITMENT_MESSAGE_CREATE_TYPE,
        format!(
            "auction-recruitment:{}:initial-message",
            snapshot.recruitment.id
        ),
        prepared.auction_recruitment_notification_id,
        prepared.now,
    )
    .await?;

    let source_id = command.source_recruitment_id.to_string();
    let auction_recruitment_id = prepared.auction_recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "AUCTION_RECRUITMENT_CREATED_FROM_ENTRY_DRAFT",
            target_type: "AUCTION_RECRUITMENT",
            target_id: &auction_recruitment_id,
            request_id,
            before_summary: Some(json!({
                "sourceRecruitmentId": command.source_recruitment_id,
                "sourceRecruitmentRevision": recruitment.revision,
                "participantCount": participants.len(),
                "declaredLaneCount": participants
                    .iter()
                    .filter(|participant| participant.declared_lane.is_some())
                    .count(),
            })),
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.regular_recruitment_outbox_event_id,
        format!(
            "guild:{guild_id}:recruitment:{}",
            command.source_recruitment_id
        ),
        "RECRUITMENT",
        &source_id,
        converted_revision,
        "RECRUITMENT_CONVERTED",
        json!({
            "type": "invalidate",
            "resourceId": command.source_recruitment_id,
            "revision": converted_revision,
        }),
        prepared.now,
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.auction_recruitment_outbox_event_id,
        format!(
            "guild:{guild_id}:auction-recruitment:{}",
            prepared.auction_recruitment_id
        ),
        "AUCTION_RECRUITMENT",
        &auction_recruitment_id,
        snapshot.recruitment.revision,
        "AUCTION_RECRUITMENT_CREATED",
        json!({
            "type": "invalidate",
            "resourceId": prepared.auction_recruitment_id,
            "revision": snapshot.recruitment.revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 201, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_series_pick_ban_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    request_id: &str,
    command: &WebSeriesPickBanCommand,
    prepared: WebSeriesPickBanPrepared,
) -> Result<SeriesPickBanSnapshot, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;

    let series = lock_series(transaction, guild_id, command.series_id).await?;
    if series.revision != command.expected_revision {
        return Err(AttemptError::SeriesRevisionConflict {
            expected: command.expected_revision,
            actual: series.revision,
        });
    }
    ensure_series_in_progress(series.status)?;

    let confirmed_game_count =
        confirmed_series_game_count(transaction, guild_id, command.series_id).await?;
    let set_number = next_series_set_number(confirmed_game_count, series.best_of)?;
    let roster = locked_series_roster(transaction, guild_id, command.series_id).await?;
    ensure_complete_series_roster_rows(&roster)?;
    let input_id = ensure_locked_series_draft_input(
        transaction,
        guild_id,
        command.series_id,
        set_number,
        prepared.input_id,
        actor_id,
        prepared.now,
    )
    .await?;
    apply_series_pick_ban_operation(transaction, guild_id, input_id, &roster, &command.operation)
        .await?;
    let draft_updated = sqlx::query(
        "UPDATE game_inputs
         SET updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND id = $4 AND status = 'DRAFT'",
    )
    .bind(actor_id)
    .bind(prepared.now)
    .bind(guild_id)
    .bind(input_id)
    .execute(&mut **transaction)
    .await?;
    if draft_updated.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }
    validate_series_pick_ban_draft(
        transaction,
        guild_id,
        command.series_id,
        series.hard_fearless,
    )
    .await?;

    let next_revision = increment_series_revision(
        transaction,
        guild_id,
        command.series_id,
        actor_id,
        prepared.now,
        SeriesStatus::InProgress,
        None,
        None,
        None,
    )
    .await?;
    let snapshot =
        series_pick_ban_snapshot(transaction, guild_id, command.series_id, actor_id).await?;
    if snapshot.series.revision != next_revision {
        return Err(AttemptError::InvariantViolation);
    }
    let series_id = command.series_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "SERIES_PICKBAN_UPDATED",
            target_type: "SERIES_PICKBAN_DRAFT",
            target_id: &series_id,
            request_id,
            before_summary: Some(json!({
                "revision": series.revision,
                "setNumber": set_number,
                "hardFearless": series.hard_fearless,
            })),
            after_summary: Some(json!({
                "revision": next_revision,
                "operation": command.operation,
            })),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:series:{}", command.series_id),
        "SERIES",
        &series_id,
        next_revision,
        "SERIES_PICKBAN_UPDATED",
        json!({
            "type": "invalidate",
            "resourceId": command.series_id,
            "revision": next_revision,
        }),
        prepared.now,
    )
    .await?;
    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_series_game_submit_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebSeriesGameSubmitCommand,
    prepared: WebSeriesGameMutationPrepared,
) -> Result<SeriesGameMutationResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<SeriesGameMutationResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: SERIES_GAME_SUBMIT_METHOD,
            route_template: SERIES_GAME_SUBMIT_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let series = lock_series(transaction, guild_id, command.series_id).await?;
    if series.revision != command.command.expected_revision {
        return Err(AttemptError::SeriesRevisionConflict {
            expected: command.command.expected_revision,
            actual: series.revision,
        });
    }
    ensure_series_in_progress(series.status)?;
    let roster = locked_series_roster(transaction, guild_id, command.series_id).await?;
    ensure_complete_series_roster_rows(&roster)?;
    let confirmed_game_count =
        confirmed_series_game_count(transaction, guild_id, command.series_id).await?;
    let expected_set_number = next_series_set_number(confirmed_game_count, series.best_of)?;
    if command.command.set_number != expected_set_number {
        return Err(AttemptError::SeriesSetNotCurrent {
            expected: expected_set_number,
            actual: command.command.set_number,
        });
    }
    let input_id = ensure_locked_series_draft_input(
        transaction,
        guild_id,
        command.series_id,
        expected_set_number,
        prepared.input_id,
        actor_id,
        prepared.now,
    )
    .await?;
    replace_series_draft_from_submit(transaction, guild_id, input_id, &roster, &command.command)
        .await?;
    validate_series_pick_ban_draft(
        transaction,
        guild_id,
        command.series_id,
        series.hard_fearless,
    )
    .await?;

    let input = sqlx::query_as::<_, SeriesPickBanGameInputRow>(
        "SELECT id, set_number, side_preset, side_selected, winning_side, patch_version
         FROM game_inputs
         WHERE guild_id = $1 AND id = $2 AND status = 'DRAFT'
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(input_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)?;
    let winner = input
        .winning_side
        .as_deref()
        .map(canonical_blue_red_side)
        .transpose()?
        .ok_or(AttemptError::InvalidGameResult)?;
    let side_preset = canonical_side_preset(&input.side_preset)?;
    let winner_team = winner_roster_team(&side_preset, &winner)?;

    let picks = series_input_picks(transaction, guild_id, input_id, &roster).await?;
    let bans = series_input_bans(transaction, guild_id, input_id).await?;
    let season_id = series_season_id(transaction, guild_id, command.series_id).await?;
    sqlx::query(
        "INSERT INTO games (
             guild_id, id, season_id, series_id, auction_match_id, source_input_id, game_number,
             side_preset, winning_side, duration_seconds, patch_version, riot_match_id, status,
             mmr_status, played_at, confirmed_by, confirmed_by_type, confirmed_at,
             voided_by, voided_by_type, voided_at, void_reason
         ) VALUES (
             $1, $2, $3, $4, NULL, $5, $6,
             $7, $8, NULL, $9, NULL, 'CONFIRMED', 'APPLIED', $10, $11, 'USER', $10,
             NULL, NULL, NULL, NULL
         )",
    )
    .bind(guild_id)
    .bind(prepared.game_id.as_uuid())
    .bind(season_id.as_uuid())
    .bind(command.series_id.as_uuid())
    .bind(input_id)
    .bind(input.set_number)
    .bind(&side_preset)
    .bind(&winner)
    .bind(&input.patch_version)
    .bind(prepared.now)
    .bind(actor_id)
    .execute(&mut **transaction)
    .await?;
    insert_series_game_participants(transaction, guild_id, prepared.game_id, &roster, &picks)
        .await?;
    insert_series_game_bans(transaction, guild_id, prepared.game_id, &bans).await?;
    apply_series_game_mmr(
        transaction,
        guild_id,
        season_id,
        prepared.game_id,
        &roster,
        winner_team,
        actor_id,
        request_id,
        prepared.now,
    )
    .await?;
    let submitted = sqlx::query(
        "UPDATE game_inputs
         SET status = 'SUBMITTED', updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND id = $4 AND status = 'DRAFT'",
    )
    .bind(actor_id)
    .bind(prepared.now)
    .bind(guild_id)
    .bind(input_id)
    .execute(&mut **transaction)
    .await?;
    if submitted.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    let (team_1_score, team_2_score) =
        confirmed_series_score(transaction, guild_id, command.series_id).await?;
    let required_wins = i64::from(series.best_of / 2 + 1);
    let (status, winning_team, terminal_reason, ended_at) = if team_1_score >= required_wins {
        (
            SeriesStatus::Completed,
            Some("TEAM_1"),
            Some("SCORE_REACHED"),
            Some(prepared.now),
        )
    } else if team_2_score >= required_wins {
        (
            SeriesStatus::Completed,
            Some("TEAM_2"),
            Some("SCORE_REACHED"),
            Some(prepared.now),
        )
    } else {
        (SeriesStatus::InProgress, None, None, None)
    };
    let revision = increment_series_revision(
        transaction,
        guild_id,
        command.series_id,
        actor_id,
        prepared.now,
        status,
        winning_team,
        terminal_reason,
        ended_at,
    )
    .await?;
    let confirmed_game_count = confirmed_game_count + 1;
    let result = SeriesGameMutationResult {
        series: SeriesGameMutationSeries {
            id: command.series_id,
            status,
            revision,
        },
        confirmed_game_count,
        next_href: if status == SeriesStatus::Completed {
            format!("/g/{guild_id}/series/{}/result", command.series_id)
        } else {
            format!("/g/{guild_id}/series/{}/pick-ban", command.series_id)
        },
    };
    if status == SeriesStatus::Completed {
        enqueue_series_result_notification_if_configured(
            transaction,
            guild_id,
            command.series_id,
            revision,
            prepared.result_notification_id,
            prepared.now,
        )
        .await?;
    }
    let series_id = command.series_id.to_string();
    let game_id = prepared.game_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "SERIES_GAME_CONFIRMED",
            target_type: "SERIES_GAME",
            target_id: &game_id,
            request_id,
            before_summary: Some(json!({
                "seriesId": command.series_id,
                "seriesRevision": series.revision,
                "setNumber": input.set_number,
            })),
            after_summary: Some(serde_json::to_value(&result)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:series:{}", command.series_id),
        "SERIES",
        &series_id,
        revision,
        "SERIES_GAME_CONFIRMED",
        json!({
            "type": "invalidate",
            "resourceId": command.series_id,
            "revision": revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 201, &result, prepared.now).await?;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_series_game_revert_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebSeriesGameRevertCommand,
    prepared: WebSeriesGameMutationPrepared,
) -> Result<SeriesGameMutationResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<SeriesGameMutationResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: SERIES_GAME_REVERT_METHOD,
            route_template: SERIES_GAME_REVERT_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };
    let series = lock_series(transaction, guild_id, command.series_id).await?;
    if series.revision != command.command.expected_revision {
        return Err(AttemptError::SeriesRevisionConflict {
            expected: command.command.expected_revision,
            actual: series.revision,
        });
    }
    ensure_series_game_revertible(series.status)?;
    let reverted = void_last_confirmed_series_game(
        transaction,
        guild_id,
        command.series_id,
        actor_id,
        request_id,
        prepared.now,
    )
    .await?;
    let revision = increment_series_revision(
        transaction,
        guild_id,
        command.series_id,
        actor_id,
        prepared.now,
        SeriesStatus::InProgress,
        None,
        None,
        None,
    )
    .await?;
    let confirmed_game_count =
        confirmed_series_game_count(transaction, guild_id, command.series_id).await?;
    let result = SeriesGameMutationResult {
        series: SeriesGameMutationSeries {
            id: command.series_id,
            status: SeriesStatus::InProgress,
            revision,
        },
        confirmed_game_count,
        next_href: format!("/g/{guild_id}/series/{}/pick-ban", command.series_id),
    };
    let series_id = command.series_id.to_string();
    let game_id = reverted.game_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "SERIES_GAME_REVERTED",
            target_type: "SERIES_GAME",
            target_id: &game_id,
            request_id,
            before_summary: Some(json!({
                "seriesId": command.series_id,
                "seriesRevision": series.revision,
                "setNumber": reverted.set_number,
            })),
            after_summary: Some(serde_json::to_value(&result)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:series:{}", command.series_id),
        "SERIES",
        &series_id,
        revision,
        "SERIES_GAME_REVERTED",
        json!({
            "type": "invalidate",
            "resourceId": command.series_id,
            "revision": revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_series_early_complete_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebSeriesEarlyCompleteCommand,
    prepared: WebSeriesEarlyCompletePrepared,
) -> Result<SeriesGameMutationResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<SeriesGameMutationResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: SERIES_EARLY_COMPLETE_METHOD,
            route_template: SERIES_EARLY_COMPLETE_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let series = lock_series(transaction, guild_id, command.series_id).await?;
    if series.revision != command.command.expected_revision {
        return Err(AttemptError::SeriesRevisionConflict {
            expected: command.command.expected_revision,
            actual: series.revision,
        });
    }
    ensure_series_in_progress(series.status)?;

    let confirmed_game_count =
        confirmed_series_game_count(transaction, guild_id, command.series_id).await?;
    if !can_early_complete_series(series.best_of, confirmed_game_count) {
        return Err(AttemptError::SeriesEarlyCompletionUnavailable {
            best_of: series.best_of,
            confirmed_game_count,
        });
    }
    let (team_1_score, team_2_score) =
        confirmed_series_score(transaction, guild_id, command.series_id).await?;
    let (winning_team, terminal_reason) = match command.command.outcome {
        SeriesEarlyCompletionOutcome::Team1 => (Some("TEAM_1"), "EARLY_WIN"),
        SeriesEarlyCompletionOutcome::Team2 => (Some("TEAM_2"), "EARLY_WIN"),
        SeriesEarlyCompletionOutcome::Draw => {
            if team_1_score != team_2_score {
                return Err(AttemptError::SeriesEarlyDrawRequiresTiedScore {
                    team_1_score,
                    team_2_score,
                });
            }
            (None, "EARLY_DRAW")
        }
    };
    let revision = increment_series_revision(
        transaction,
        guild_id,
        command.series_id,
        actor_id,
        prepared.now,
        SeriesStatus::Completed,
        winning_team,
        Some(terminal_reason),
        Some(prepared.now),
    )
    .await?;
    let result = SeriesGameMutationResult {
        series: SeriesGameMutationSeries {
            id: command.series_id,
            status: SeriesStatus::Completed,
            revision,
        },
        confirmed_game_count,
        next_href: format!("/g/{guild_id}/series/{}/result", command.series_id),
    };
    enqueue_series_result_notification_if_configured(
        transaction,
        guild_id,
        command.series_id,
        revision,
        prepared.result_notification_id,
        prepared.now,
    )
    .await?;
    let series_id = command.series_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "SERIES_EARLY_COMPLETED",
            target_type: "SERIES",
            target_id: &series_id,
            request_id,
            before_summary: Some(json!({
                "seriesRevision": series.revision,
                "bestOf": series.best_of,
                "confirmedGameCount": confirmed_game_count,
                "score": { "team1": team_1_score, "team2": team_2_score },
            })),
            after_summary: Some(json!({
                "seriesRevision": revision,
                "status": "COMPLETED",
                "outcome": command.command.outcome,
                "winningTeam": winning_team,
                "terminalReason": terminal_reason,
                "confirmedGameCount": confirmed_game_count,
                "nextHref": result.next_href,
            })),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:series:{}", command.series_id),
        "SERIES",
        &series_id,
        revision,
        "SERIES_EARLY_COMPLETED",
        json!({
            "type": "invalidate",
            "resourceId": command.series_id,
            "revision": revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_series_lifecycle_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebSeriesLifecycleCommand,
    prepared: WebSeriesLifecyclePrepared,
) -> Result<SeriesLifecycleResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;

    let metadata = command.action.metadata();
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<SeriesLifecycleResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    // A lifecycle mutation always follows the documented source recruitment -> series lock order.
    let source_recruitment_id =
        resolve_series_source_recruitment(transaction, guild_id, command.series_id).await?;
    let recruitment = lock_recruitment(transaction, guild_id, source_recruitment_id).await?;
    let series = lock_series(transaction, guild_id, command.series_id).await?;
    if series.source_recruitment_id != source_recruitment_id {
        return Err(AttemptError::InvariantViolation);
    }
    if series.revision != command.expected_revision {
        return Err(AttemptError::SeriesRevisionConflict {
            expected: command.expected_revision,
            actual: series.revision,
        });
    }
    ensure_series_game_revertible(series.status)?;
    ensure_series_source_converted(recruitment.status)?;

    let confirmed_game_count =
        confirmed_series_game_count(transaction, guild_id, command.series_id).await?;
    for _ in 0..confirmed_game_count {
        void_last_confirmed_series_game(
            transaction,
            guild_id,
            command.series_id,
            actor_id,
            request_id,
            prepared.now,
        )
        .await?;
    }

    let series_revision = series
        .revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;
    let recruitment_revision = recruitment
        .revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;
    let cancelled = sqlx::query(
        "UPDATE series
         SET status = 'CANCELLED',
             winning_team = NULL,
             terminal_reason = 'CANCELLED',
             revision = revision + 1,
             ended_at = $1,
             updated_by = $2,
             updated_at = $1
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(prepared.now)
    .bind(actor_id)
    .bind(guild_id)
    .bind(command.series_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if cancelled.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    let next_href = match command.action {
        SeriesLifecycleAction::Revert => {
            let restored = sqlx::query(
                "UPDATE recruitments
                 SET status = 'CLOSED',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     converted_at = NULL,
                     cancelled_at = NULL
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(source_recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            if restored.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
            format!("/g/{guild_id}/recruitments/{source_recruitment_id}/entry")
        }
        SeriesLifecycleAction::Delete => {
            let deleted = sqlx::query(
                "UPDATE recruitments
                 SET status = 'CANCELLED',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     converted_at = NULL,
                     cancelled_at = $2
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(source_recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            if deleted.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
            format!("/g/{guild_id}/series")
        }
    };
    let result = SeriesLifecycleResult { next_href };
    let series_id = command.series_id.to_string();
    let source_id = source_recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: metadata.audit_action,
            target_type: "SERIES",
            target_id: &series_id,
            request_id,
            before_summary: Some(json!({
                "seriesRevision": series.revision,
                "seriesStatus": series.status.as_str(),
                "confirmedGameCount": confirmed_game_count,
                "sourceRecruitmentId": source_recruitment_id,
                "sourceRecruitmentRevision": recruitment.revision,
                "sourceRecruitmentStatus": recruitment.status.as_str(),
            })),
            after_summary: Some(json!({
                "seriesRevision": series_revision,
                "seriesStatus": "CANCELLED",
                "revertedGameCount": confirmed_game_count,
                "sourceRecruitmentId": source_recruitment_id,
                "sourceRecruitmentRevision": recruitment_revision,
                "sourceRecruitmentStatus": metadata.source_status,
                "nextHref": result.next_href,
            })),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.series_outbox_event_id,
        format!("guild:{guild_id}:series:{}", command.series_id),
        "SERIES",
        &series_id,
        series_revision,
        metadata.series_event_type,
        json!({
            "type": "invalidate",
            "resourceId": command.series_id,
            "revision": series_revision,
        }),
        prepared.now,
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.recruitment_outbox_event_id,
        format!("guild:{guild_id}:recruitment:{source_recruitment_id}"),
        "RECRUITMENT",
        &source_id,
        recruitment_revision,
        metadata.recruitment_event_type,
        json!({
            "type": "invalidate",
            "resourceId": source_recruitment_id,
            "revision": recruitment_revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn execute_recruitment_create_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor: &RecruitmentCreateActor,
    rules: RecruitmentSeriesRules,
    expected_start_at: OffsetDateTime,
    principal_type: IdempotencyPrincipalType,
    principal_id: &str,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    prepared: RecruitmentPrepared,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let _operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<RecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type,
            principal_id,
            method: "POST",
            route_template: "/api/guilds/{guildId}/recruitments",
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let actor_id = actor.user_id();
    if let RecruitmentCreateActor::Bot(actor) = actor {
        sync_guild_actor(transaction, guild_id, actor, prepared.now).await?;
    }
    require_active_member_for_creation(transaction, guild_id, &actor_id).await?;
    let recruitment_channel_id = match actor {
        RecruitmentCreateActor::Bot(_) => None,
        RecruitmentCreateActor::Session(_) => {
            Some(require_recruitment_channel(transaction, guild_id).await?)
        }
    };
    let snapshot = insert_recruitment(
        transaction,
        guild_id,
        &actor_id,
        rules,
        expected_start_at,
        prepared.recruitment_id,
        prepared.now,
    )
    .await?;

    if let Some(channel_id) = recruitment_channel_id.as_ref() {
        enqueue_recruitment_message_notification(
            transaction,
            guild_id,
            channel_id,
            &snapshot,
            RECRUITMENT_MESSAGE_CREATE_TYPE,
            format!("recruitment:{}:initial-message", snapshot.recruitment.id),
            prepared.notification_id,
            prepared.now,
        )
        .await?;
    }

    let recruitment_id = snapshot.recruitment.id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "RECRUITMENT_CREATE",
            target_type: "RECRUITMENT",
            target_id: &recruitment_id,
            request_id,
            before_summary: None,
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:recruitment:{}", snapshot.recruitment.id),
        "RECRUITMENT",
        &recruitment_id,
        snapshot.recruitment.revision,
        "RECRUITMENT_CREATED",
        json!({
            "type": "invalidate",
            "resourceId": snapshot.recruitment.id,
            "revision": snapshot.recruitment.revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 201, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_auction_recruitment_create_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor: &AuctionRecruitmentCreateActor,
    format: AuctionFormat,
    rules: AuctionRecruitmentSeriesRules,
    expected_start_at: OffsetDateTime,
    principal_type: IdempotencyPrincipalType,
    principal_id: &str,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    prepared: AuctionRecruitmentPrepared,
) -> Result<AuctionRecruitmentSnapshot, AttemptError> {
    let _operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionRecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type,
            principal_id,
            method: "POST",
            route_template: AUCTION_RECRUITMENT_CREATE_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let actor_id = actor.user_id();
    if let AuctionRecruitmentCreateActor::Bot(actor) = actor {
        sync_guild_actor(transaction, guild_id, actor, prepared.now).await?;
    }
    require_active_member_for_creation(transaction, guild_id, &actor_id).await?;
    let channel_id = require_recruitment_channel(transaction, guild_id).await?;
    let snapshot = insert_auction_recruitment(
        transaction,
        guild_id,
        &actor_id,
        format,
        rules,
        expected_start_at,
        prepared.recruitment_id,
        prepared.now,
    )
    .await?;

    enqueue_auction_recruitment_message_notification(
        transaction,
        guild_id,
        &channel_id,
        &snapshot,
        AUCTION_RECRUITMENT_MESSAGE_CREATE_TYPE,
        format!(
            "auction-recruitment:{}:initial-message",
            snapshot.recruitment.id
        ),
        prepared.notification_id,
        prepared.now,
    )
    .await?;

    let recruitment_id = snapshot.recruitment.id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "AUCTION_RECRUITMENT_CREATE",
            target_type: "AUCTION_RECRUITMENT",
            target_id: &recruitment_id,
            request_id,
            before_summary: None,
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!(
            "guild:{guild_id}:auction-recruitment:{}",
            snapshot.recruitment.id
        ),
        "AUCTION_RECRUITMENT",
        &recruitment_id,
        snapshot.recruitment.revision,
        "AUCTION_RECRUITMENT_CREATED",
        json!({
            "type": "invalidate",
            "resourceId": snapshot.recruitment.id,
            "revision": snapshot.recruitment.revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 201, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

async fn execute_auction_recruitment_participation_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &str,
    request_hash: &[u8],
    command: &BotAuctionRecruitmentParticipationCommand,
    prepared: AuctionRecruitmentMutationPrepared,
) -> Result<AuctionRecruitmentSnapshot, AttemptError> {
    let metadata = command.metadata();
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionRecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key.as_str(),
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let actor = command.actor();
    let actor_id = UserId::from(&actor.id);
    sync_guild_actor(transaction, guild_id, actor, prepared.now).await?;
    require_active_member(transaction, guild_id, &actor.id).await?;

    let (before, snapshot) = match command {
        BotAuctionRecruitmentParticipationCommand::Join {
            participant,
            recruitment_id,
            declared_lane,
            ..
        } => {
            let participant_id = UserId::from(&participant.id);
            require_self_or_operator(actor, &participant_id, &operator_role_id)?;
            let locked = lock_auction_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            if actor.id != participant.id {
                sync_guild_actor(transaction, guild_id, participant, prepared.now).await?;
                require_active_member(transaction, guild_id, &participant.id).await?;
            }
            let lane =
                auction_join_lane(transaction, guild_id, &participant_id, *declared_lane).await?;
            let mmr_rows = mmr::ensure_and_lock_mmr_rows(
                transaction,
                guild_id,
                SeasonId::from_uuid(locked.season_id),
                [MmrLaneKey {
                    user_id: participant_id.clone(),
                    lane,
                }],
                prepared.now,
            )
            .await?;
            let mmr_at_join = mmr_rows
                .into_iter()
                .next()
                .ok_or(AttemptError::InvariantViolation)?
                .mmr;
            let before =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, &actor_id)
                    .await?;
            join_auction_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                &participant_id,
                lane,
                mmr_at_join,
                locked.target_count,
                prepared.now,
            )
            .await?;
            bump_auction_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                &actor_id,
                prepared.now,
            )
            .await?;
            let snapshot =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, &actor_id)
                    .await?;
            (Some(before), snapshot)
        }
        BotAuctionRecruitmentParticipationCommand::Leave {
            recruitment_id,
            user_id,
            ..
        } => {
            require_self_or_operator(actor, user_id, &operator_role_id)?;
            let locked = lock_auction_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            let before =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, &actor_id)
                    .await?;
            leave_auction_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                user_id,
                prepared.now,
            )
            .await?;
            bump_auction_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                &actor_id,
                prepared.now,
            )
            .await?;
            let snapshot =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, &actor_id)
                    .await?;
            (Some(before), snapshot)
        }
    };

    let recruitment_id = snapshot.recruitment.id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor.id.as_str(),
            action: metadata.audit_action,
            target_type: "AUCTION_RECRUITMENT",
            target_id: &recruitment_id,
            request_id,
            before_summary: before.as_ref().map(serde_json::to_value).transpose()?,
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!(
            "guild:{guild_id}:auction-recruitment:{}",
            snapshot.recruitment.id
        ),
        "AUCTION_RECRUITMENT",
        &recruitment_id,
        snapshot.recruitment.revision,
        metadata.event_type,
        json!({
            "type": "invalidate",
            "resourceId": snapshot.recruitment.id,
            "revision": snapshot.recruitment.revision,
        }),
        prepared.now,
    )
    .await?;
    enqueue_auction_recruitment_message_update(
        transaction,
        guild_id,
        &snapshot,
        prepared.notification_id,
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_auction_recruitment_mutation_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebAuctionRecruitmentMutation,
    prepared: WebAuctionRecruitmentMutationPrepared,
) -> Result<AuctionRecruitmentSnapshot, AttemptError> {
    let metadata = command.metadata();
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionRecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let (before, snapshot) = match command {
        WebAuctionRecruitmentMutation::AddMember {
            recruitment_id,
            member_id,
            declared_lane,
        } => {
            let locked = lock_auction_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            require_active_recruitment_member(transaction, guild_id, member_id).await?;
            let before =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, actor_id)
                    .await?;
            let mmr_rows = mmr::ensure_and_lock_mmr_rows(
                transaction,
                guild_id,
                SeasonId::from_uuid(locked.season_id),
                [MmrLaneKey {
                    user_id: member_id.clone(),
                    lane: *declared_lane,
                }],
                prepared.now,
            )
            .await?;
            let mmr_at_join = mmr_rows
                .into_iter()
                .next()
                .ok_or(AttemptError::InvariantViolation)?
                .mmr;
            join_auction_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                member_id,
                *declared_lane,
                mmr_at_join,
                locked.target_count,
                prepared.now,
            )
            .await?;
            bump_auction_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
            let snapshot =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, actor_id)
                    .await?;
            (Some(before), snapshot)
        }
        WebAuctionRecruitmentMutation::RemoveMember {
            recruitment_id,
            member_id,
        } => {
            let locked = lock_auction_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            let before =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, actor_id)
                    .await?;
            leave_auction_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                member_id,
                prepared.now,
            )
            .await?;
            bump_auction_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
            let snapshot =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, actor_id)
                    .await?;
            (Some(before), snapshot)
        }
    };

    let recruitment_id = snapshot.recruitment.id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: metadata.audit_action,
            target_type: "AUCTION_RECRUITMENT",
            target_id: &recruitment_id,
            request_id,
            before_summary: before.as_ref().map(serde_json::to_value).transpose()?,
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!(
            "guild:{guild_id}:auction-recruitment:{}",
            snapshot.recruitment.id
        ),
        "AUCTION_RECRUITMENT",
        &recruitment_id,
        snapshot.recruitment.revision,
        metadata.event_type,
        json!({
            "type": "invalidate",
            "resourceId": snapshot.recruitment.id,
            "revision": snapshot.recruitment.revision,
        }),
        prepared.now,
    )
    .await?;
    enqueue_auction_recruitment_message_update(
        transaction,
        guild_id,
        &snapshot,
        prepared.notification_id,
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_auction_recruitment_lifecycle_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebAuctionRecruitmentLifecycle,
    prepared: WebAuctionRecruitmentLifecyclePrepared,
) -> Result<AuctionRecruitmentSnapshot, AttemptError> {
    let metadata = command.metadata();
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionRecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let (before, snapshot) = match command {
        WebAuctionRecruitmentLifecycle::Close { recruitment_id } => {
            require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id)
                .await?;
            let locked = lock_auction_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            let joined = joined_auction_recruitment_participant_count(
                transaction,
                guild_id,
                *recruitment_id,
            )
            .await?;
            if joined != i64::from(locked.target_count) {
                return Err(AttemptError::CloseRequiresFullRoster {
                    joined,
                    target: locked.target_count,
                });
            }
            let before =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, actor_id)
                    .await?;
            sqlx::query(
                "UPDATE auction_recruitments
                 SET status = 'CLOSED',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     closed_at = $2
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            let snapshot =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, actor_id)
                    .await?;
            (Some(before), snapshot)
        }
        WebAuctionRecruitmentLifecycle::Cancel { recruitment_id } => {
            let locked = lock_auction_recruitment(transaction, guild_id, *recruitment_id).await?;
            require_active_creator_or_operator(
                transaction,
                guild_id,
                actor_id,
                &locked.created_by,
                &operator_role_id,
            )
            .await?;
            ensure_open(locked.status)?;
            let before =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, actor_id)
                    .await?;
            sqlx::query(
                "UPDATE auction_recruitments
                 SET status = 'CANCELLED',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     cancelled_at = $2
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            let snapshot =
                auction_recruitment_snapshot(transaction, guild_id, *recruitment_id, actor_id)
                    .await?;
            (Some(before), snapshot)
        }
    };

    let recruitment_id = snapshot.recruitment.id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: metadata.audit_action,
            target_type: "AUCTION_RECRUITMENT",
            target_id: &recruitment_id,
            request_id,
            before_summary: before.as_ref().map(serde_json::to_value).transpose()?,
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!(
            "guild:{guild_id}:auction-recruitment:{}",
            snapshot.recruitment.id
        ),
        "AUCTION_RECRUITMENT",
        &recruitment_id,
        snapshot.recruitment.revision,
        metadata.event_type,
        json!({
            "type": "invalidate",
            "resourceId": snapshot.recruitment.id,
            "revision": snapshot.recruitment.revision,
        }),
        prepared.now,
    )
    .await?;
    enqueue_auction_recruitment_message_lifecycle_notification(
        transaction,
        guild_id,
        &snapshot,
        command,
        prepared.notification_id,
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_auction_recruitment_participant_confirmation_dm_dispatch_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    prepared: WebParticipantConfirmationDmDispatchPrepared,
) -> Result<ParticipantConfirmationDmDispatchResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<ParticipantConfirmationDmDispatchResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: "POST",
            route_template: AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DMS_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let locked = lock_auction_recruitment(transaction, guild_id, recruitment_id).await?;
    ensure_participant_confirmation_dms_dispatchable(locked.status)?;
    let snapshot =
        auction_recruitment_snapshot(transaction, guild_id, recruitment_id, actor_id).await?;
    let result = enqueue_auction_recruitment_participant_confirmation_direct_messages(
        transaction,
        guild_id,
        &snapshot,
        prepared.now,
    )
    .await?;
    let target_id = recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DMS_REQUESTED",
            target_type: "AUCTION_RECRUITMENT",
            target_id: &target_id,
            request_id,
            before_summary: None,
            after_summary: Some(serde_json::to_value(&result)?),
            now: prepared.now,
        },
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_auction_setup_draft_update_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebAuctionSetupDraftUpdateCommand,
    prepared: WebAuctionSetupDraftPrepared,
) -> Result<AuctionSetupDraftSnapshot, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;

    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionSetupDraftSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: "PATCH",
            route_template: AUCTION_SETUP_DRAFT_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    // Keep this order aligned with tournament creation and future auction commands.
    let recruitment =
        lock_auction_recruitment(transaction, guild_id, command.recruitment_id).await?;
    ensure_auction_setup_editable(recruitment.status)?;
    let actual_revision =
        lock_auction_setup_draft_revision(transaction, guild_id, command.recruitment_id).await?;
    let locked_teams =
        lock_auction_setup_teams(transaction, guild_id, command.recruitment_id).await?;
    let joined =
        lock_joined_auction_recruitment_participants(transaction, guild_id, command.recruitment_id)
            .await?;
    ensure_full_auction_roster(&joined, recruitment.target_count)?;
    if actual_revision != command.expected_revision {
        return Err(AttemptError::AuctionSetupDraftRevisionConflict {
            expected: command.expected_revision,
            actual: actual_revision,
        });
    }
    let format = parse_auction_format(&recruitment.format)?;
    let normalized =
        normalize_auction_setup_team_inputs(&command.teams, format.team_count(), &joined)?;
    if locked_teams.len() != usize::try_from(format.team_count()).unwrap_or_default() {
        return Err(AttemptError::MissingAuctionSetupDraft);
    }
    let before =
        auction_setup_draft_snapshot(transaction, guild_id, command.recruitment_id, actor_id)
            .await?;

    for team in &normalized {
        let updated = sqlx::query(
            "UPDATE auction_setup_teams
             SET team_name = $1, captain_user_id = $2, initial_points = $3
             WHERE guild_id = $4 AND recruitment_id = $5 AND team_index = $6",
        )
        .bind(&team.team_name)
        .bind(&team.captain_user_id)
        .bind(team.initial_points)
        .bind(guild_id)
        .bind(command.recruitment_id.as_uuid())
        .bind(team.team_index)
        .execute(&mut **transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(AttemptError::MissingAuctionSetupDraft);
        }
    }
    let updated_revision = sqlx::query_scalar::<_, i64>(
        "UPDATE auction_setup_drafts
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND recruitment_id = $4
         RETURNING revision",
    )
    .bind(actor_id)
    .bind(prepared.now)
    .bind(guild_id)
    .bind(command.recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingAuctionSetupDraft)?;
    if updated_revision != actual_revision + 1 {
        return Err(AttemptError::InvariantViolation);
    }

    let snapshot =
        auction_setup_draft_snapshot(transaction, guild_id, command.recruitment_id, actor_id)
            .await?;
    let target_id = command.recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "AUCTION_SETUP_DRAFT_UPDATED",
            target_type: "AUCTION_SETUP_DRAFT",
            target_id: &target_id,
            request_id,
            before_summary: Some(serde_json::to_value(&before)?),
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_auction_tournament_create_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebAuctionTournamentCreateCommand,
    prepared: WebAuctionTournamentCreatePrepared,
) -> Result<AuctionTournamentDraftSnapshot, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;

    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionTournamentDraftSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: "POST",
            route_template: AUCTION_TOURNAMENT_CREATE_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    // Source recruitment serializes conversion. Lock the dependent draft and teams in order.
    let recruitment =
        lock_auction_recruitment(transaction, guild_id, command.source_recruitment_id).await?;
    ensure_auction_tournament_source_ready(recruitment.status)?;
    let actual_revision =
        lock_auction_setup_draft_revision(transaction, guild_id, command.source_recruitment_id)
            .await?;
    let setup_teams =
        lock_auction_setup_teams(transaction, guild_id, command.source_recruitment_id).await?;
    let joined = lock_joined_auction_recruitment_participants(
        transaction,
        guild_id,
        command.source_recruitment_id,
    )
    .await?;
    ensure_full_auction_roster(&joined, recruitment.target_count)?;
    if actual_revision != command.expected_revision {
        return Err(AttemptError::AuctionSetupDraftRevisionConflict {
            expected: command.expected_revision,
            actual: actual_revision,
        });
    }
    let format = parse_auction_format(&recruitment.format)?;
    let setup_teams =
        validate_stored_auction_setup_teams(&setup_teams, format.team_count(), &joined)?;
    let existing = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM auction_tournaments
             WHERE guild_id = $1
               AND source_recruitment_id = $2
               AND status <> 'CANCELLED'
               AND deleted_at IS NULL
         )",
    )
    .bind(guild_id)
    .bind(command.source_recruitment_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?;
    if existing {
        return Err(AttemptError::AuctionTournamentAlreadyExists);
    }

    let display_number = next_auction_tournament_display_number(transaction, guild_id).await?;
    let title = format!(
        "{}인 경매 내전 #{display_number}",
        format.participant_count()
    );
    sqlx::query(
        "INSERT INTO auction_tournaments (
             guild_id, id, season_id, source_recruitment_id, display_number, title, format,
             semifinal_best_of, final_best_of, hard_fearless, status, revision,
             bracket_locked_at, bracket_locked_by, champion_team_id, started_at, ended_at,
             created_by, updated_by, updated_at, end_card_channel_id, end_card_message_id, deleted_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7,
             $8, $9, $10, 'BIDDING', 0,
             NULL, NULL, NULL, $11, NULL,
             $12, $12, $11, NULL, NULL, NULL
         )",
    )
    .bind(guild_id)
    .bind(prepared.tournament_id.as_uuid())
    .bind(recruitment.season_id)
    .bind(command.source_recruitment_id.as_uuid())
    .bind(display_number)
    .bind(&title)
    .bind(format.as_str())
    .bind(recruitment.semifinal_best_of)
    .bind(recruitment.final_best_of)
    .bind(recruitment.hard_fearless)
    .bind(prepared.now)
    .bind(actor_id)
    .execute(&mut **transaction)
    .await?;

    let joined_by_user = joined
        .iter()
        .map(|participant| (participant.user_id.clone(), participant))
        .collect::<HashMap<_, _>>();
    let captain_ids = setup_teams
        .iter()
        .map(|team| team.captain_user_id.clone())
        .collect::<HashSet<_>>();
    for (position, team) in setup_teams.iter().enumerate() {
        let team_id = prepared.team_ids[position];
        let captain = joined_by_user
            .get(&team.captain_user_id)
            .copied()
            .ok_or(AttemptError::AuctionSetupCaptainNotJoined)?;
        sqlx::query(
            "INSERT INTO auction_teams (
                 guild_id, tournament_id, id, team_index, team_name, initial_points,
                 current_points, point_revision, created_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $6, 0, $7)",
        )
        .bind(guild_id)
        .bind(prepared.tournament_id.as_uuid())
        .bind(team_id)
        .bind(team.team_index)
        .bind(&team.team_name)
        .bind(team.initial_points)
        .bind(prepared.now)
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            "INSERT INTO auction_team_members (
                 guild_id, tournament_id, team_id, id, user_id, roster_slot, acquired_via,
                 declared_lane, mmr_at_acquisition, snapshot_origin, acquisition_action_id,
                 acquisition_action_kind, joined_at, released_at
             ) VALUES ($1, $2, $3, $4, $5, 1, 'CAPTAIN', $6, $7, $8, NULL, NULL, $9, NULL)",
        )
        .bind(guild_id)
        .bind(prepared.tournament_id.as_uuid())
        .bind(team_id)
        .bind(prepared.team_member_ids[position])
        .bind(&captain.user_id)
        .bind(captain.declared_lane)
        .bind(captain.mmr_at_join)
        .bind(&captain.snapshot_origin)
        .bind(prepared.now)
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            "INSERT INTO auction_point_ledger (
                 guild_id, tournament_id, id, team_id, point_revision, reason,
                 source_action_id, source_action_kind, points_before, points_after, delta,
                 reverses_entry_id, reversed_reason, actor_type, actor_id, created_at
             ) VALUES ($1, $2, $3, $4, 0, 'INITIAL', NULL, NULL, 0, $5, $5,
                       NULL, NULL, 'USER', $6, $7)",
        )
        .bind(guild_id)
        .bind(prepared.tournament_id.as_uuid())
        .bind(prepared.point_ledger_ids[position])
        .bind(team_id)
        .bind(team.initial_points)
        .bind(actor_id)
        .bind(prepared.now)
        .execute(&mut **transaction)
        .await?;
    }
    let bid_candidate_ids = joined
        .iter()
        .filter(|row| !captain_ids.contains(&row.user_id))
        .map(|participant| participant.user_id.clone())
        .collect::<Vec<_>>();
    let bid_candidate_order =
        randomized_auction_candidate_user_ids(transaction, &bid_candidate_ids).await?;
    for (queue_position, user_id) in bid_candidate_order.iter().enumerate() {
        let participant = joined_by_user
            .get(user_id)
            .copied()
            .ok_or(AttemptError::InvariantViolation)?;
        sqlx::query(
            "INSERT INTO auction_bid_candidates (
                 guild_id, tournament_id, user_id, declared_lane, mmr_at_start, snapshot_origin,
                 queue_position, state, draw_count, updated_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'QUEUED', 0, $8)",
        )
        .bind(guild_id)
        .bind(prepared.tournament_id.as_uuid())
        .bind(&participant.user_id)
        .bind(participant.declared_lane)
        .bind(participant.mmr_at_join)
        .bind(&participant.snapshot_origin)
        .bind(i64::try_from(queue_position).map_err(|_| AttemptError::InvariantViolation)?)
        .bind(prepared.now)
        .execute(&mut **transaction)
        .await?;
    }
    for slot_key in AUCTION_BRACKET_SLOT_KEYS {
        sqlx::query(
            "INSERT INTO auction_bracket_slots (
                 guild_id, tournament_id, slot_key, team_id, updated_by, updated_at
             ) VALUES ($1, $2, $3, NULL, NULL, $4)",
        )
        .bind(guild_id)
        .bind(prepared.tournament_id.as_uuid())
        .bind(slot_key)
        .bind(prepared.now)
        .execute(&mut **transaction)
        .await?;
    }
    let converted = sqlx::query(
        "UPDATE auction_recruitments
         SET status = 'CONVERTED', revision = revision + 1, updated_by = $1, updated_at = $2,
             converted_at = $2
         WHERE guild_id = $3 AND id = $4 AND status = 'CLOSED'",
    )
    .bind(actor_id)
    .bind(prepared.now)
    .bind(guild_id)
    .bind(command.source_recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if converted.rows_affected() != 1 {
        return Err(AttemptError::AuctionTournamentSourceNotReady(
            recruitment.status,
        ));
    }

    let snapshot =
        auction_tournament_draft_snapshot(transaction, guild_id, prepared.tournament_id, actor_id)
            .await?;
    let source_id = command.source_recruitment_id.to_string();
    let tournament_id = prepared.tournament_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "AUCTION_TOURNAMENT_CREATED",
            target_type: "AUCTION_TOURNAMENT",
            target_id: &tournament_id,
            request_id,
            before_summary: None,
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.recruitment_outbox_event_id,
        format!("guild:{guild_id}:auction-recruitment:{}", command.source_recruitment_id),
        "AUCTION_RECRUITMENT",
        &source_id,
        recruitment.revision + 1,
        "AUCTION_RECRUITMENT_CONVERTED",
        json!({"type":"invalidate","resourceId":command.source_recruitment_id,"revision":recruitment.revision + 1}),
        prepared.now,
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.tournament_outbox_event_id,
        format!("guild:{guild_id}:auction-tournament:{}", prepared.tournament_id),
        "AUCTION_TOURNAMENT",
        &tournament_id,
        snapshot.tournament.revision,
        "AUCTION_TOURNAMENT_CREATED",
        json!({"type":"invalidate","resourceId":prepared.tournament_id,"revision":snapshot.tournament.revision}),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 201, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_auction_tournament_draft_command_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &AuctionTournamentDraftCommand,
    prepared: WebAuctionTournamentDraftPrepared,
) -> Result<AuctionTournamentDraftSnapshot, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionTournamentDraftSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: "POST",
            route_template: "/api/guilds/{guildId}/auction-tournaments/{id}/commands",
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    // A tournament serializes every draft mutation. Mutable dependents then lock in stable order.
    let tournament = lock_auction_tournament(transaction, guild_id, tournament_id).await?;
    let teams = lock_auction_tournament_teams(transaction, guild_id, tournament_id).await?;
    let members =
        lock_auction_tournament_team_members(transaction, guild_id, tournament_id).await?;
    let candidates =
        lock_auction_tournament_candidates(transaction, guild_id, tournament_id).await?;
    let lots = lock_auction_tournament_lots(transaction, guild_id, tournament_id).await?;
    // These append-only ledgers inherit serialization from the tournament lock above.
    let actions = read_auction_tournament_actions(transaction, guild_id, tournament_id).await?;
    let point_ledger =
        read_auction_tournament_point_ledger(transaction, guild_id, tournament_id).await?;
    if tournament.revision != command.expected_revision() {
        return Err(AttemptError::AuctionTournamentRevisionConflict {
            expected: command.expected_revision(),
            actual: tournament.revision,
        });
    }
    ensure_auction_draft_command_allowed(
        transaction,
        guild_id,
        tournament_id,
        &tournament,
        command,
    )
    .await?;
    validate_auction_draft_state(&candidates, &lots, &teams, &members)?;
    let before =
        auction_tournament_draft_snapshot(transaction, guild_id, tournament_id, actor_id).await?;
    let next_status = apply_auction_tournament_draft_command(
        transaction,
        guild_id,
        tournament_id,
        actor_id,
        command,
        &teams,
        &members,
        &candidates,
        &lots,
        &actions,
        &point_ledger,
        prepared,
        tournament.status,
    )
    .await?;
    let revision = bump_auction_tournament_revision(
        transaction,
        guild_id,
        tournament_id,
        actor_id,
        next_status,
        prepared.now,
    )
    .await?;
    let snapshot =
        auction_tournament_draft_snapshot(transaction, guild_id, tournament_id, actor_id).await?;
    if snapshot.tournament.revision != revision {
        return Err(AttemptError::InvariantViolation);
    }
    let target_id = tournament_id.to_string();
    let audit_action = format!("AUCTION_DRAFT_{}", command.kind());
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: &audit_action,
            target_type: "AUCTION_TOURNAMENT",
            target_id: &target_id,
            request_id,
            before_summary: Some(serde_json::to_value(&before)?),
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:auction-tournament:{tournament_id}"),
        "AUCTION_TOURNAMENT",
        &target_id,
        revision,
        "AUCTION_TOURNAMENT_DRAFT_UPDATED",
        json!({"type":"invalidate","resourceId":tournament_id,"revision":revision}),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_auction_tournament_bracket_command_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &AuctionTournamentBracketCommand,
    prepared: WebAuctionTournamentBracketPrepared,
) -> Result<AuctionTournamentBracketSnapshot, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionTournamentBracketSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: "POST",
            route_template: AUCTION_TOURNAMENT_BRACKET_COMMAND_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    // The tournament row serializes every bracket edit before slot and match rows are touched.
    let tournament = lock_auction_tournament_bracket(transaction, guild_id, tournament_id).await?;
    let teams = lock_auction_tournament_teams(transaction, guild_id, tournament_id).await?;
    let members =
        lock_auction_tournament_team_members(transaction, guild_id, tournament_id).await?;
    ensure_auction_bracket_slot_rows(transaction, guild_id, tournament_id, prepared.now).await?;
    let slots = lock_auction_tournament_bracket_slots(transaction, guild_id, tournament_id).await?;
    let matches = lock_auction_tournament_matches(transaction, guild_id, tournament_id).await?;
    if tournament.revision != command.expected_revision() {
        return Err(AttemptError::AuctionTournamentRevisionConflict {
            expected: command.expected_revision(),
            actual: tournament.revision,
        });
    }
    ensure_auction_bracket_command_allowed(&tournament, &matches, command)?;
    validate_auction_bracket_slots(&slots)?;
    let before =
        auction_tournament_bracket_snapshot(transaction, guild_id, tournament_id, actor_id).await?;
    let outcome = apply_auction_tournament_bracket_command(
        transaction,
        guild_id,
        tournament_id,
        actor_id,
        command,
        &tournament,
        &teams,
        &members,
        &slots,
        prepared,
    )
    .await?;
    let revision = match outcome {
        AuctionTournamentBracketCommandOutcome::Locked => {
            mark_auction_tournament_bracket_locked(
                transaction,
                guild_id,
                tournament_id,
                actor_id,
                prepared.now,
            )
            .await?
        }
        AuctionTournamentBracketCommandOutcome::Unlocked => {
            unlock_auction_tournament_bracket(
                transaction,
                guild_id,
                tournament_id,
                actor_id,
                prepared.now,
            )
            .await?
        }
        AuctionTournamentBracketCommandOutcome::Updated => {
            bump_auction_tournament_revision(
                transaction,
                guild_id,
                tournament_id,
                actor_id,
                AuctionTournamentStatus::BracketSetup,
                prepared.now,
            )
            .await?
        }
    };
    let snapshot =
        auction_tournament_bracket_snapshot(transaction, guild_id, tournament_id, actor_id).await?;
    if snapshot.tournament.revision != revision {
        return Err(AttemptError::InvariantViolation);
    }
    let target_id = tournament_id.to_string();
    let audit_action = format!("AUCTION_BRACKET_{}", command.kind());
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: &audit_action,
            target_type: "AUCTION_TOURNAMENT",
            target_id: &target_id,
            request_id,
            before_summary: Some(serde_json::to_value(&before)?),
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:auction-tournament:{tournament_id}"),
        "AUCTION_TOURNAMENT",
        &target_id,
        revision,
        "AUCTION_TOURNAMENT_BRACKET_UPDATED",
        json!({"type":"invalidate","resourceId":tournament_id,"revision":revision}),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn cancel_auction_tournament_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    idempotency_key: &DiscordInteractionId,
    request_id: &str,
    request_hash: &[u8],
    command: &AuctionTournamentCancelCommand,
    prepared: AuctionTournamentCancelPrepared,
) -> Result<AuctionTournamentCancelResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionTournamentCancelResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
            method: AUCTION_TOURNAMENT_CANCEL_METHOD,
            route_template: AUCTION_TOURNAMENT_CANCEL_ROUTE,
            key: idempotency_key.as_str(),
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let actor = &command.actor;
    let actor_id = UserId::from(&actor.id);
    sync_guild_actor(transaction, guild_id, actor, prepared.now).await?;
    require_active_member(transaction, guild_id, &actor.id).await?;
    require_operator(actor, &operator_role_id)?;

    let cancellation = cancel_locked_auction_tournament(
        transaction,
        guild_id,
        tournament_id,
        &actor_id,
        request_id,
        prepared.now,
        Some(command.expected_revision),
        prepared.recruitment_notification_id,
    )
    .await?;
    let before = cancellation.before;
    let result = cancellation.result;
    let tournament_id_text = tournament_id.to_string();
    let recruitment_id_text = result.source_recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "AUCTION_TOURNAMENT_CANCELLED",
            target_type: "AUCTION_TOURNAMENT",
            target_id: &tournament_id_text,
            request_id,
            before_summary: Some(before),
            after_summary: Some(json!({
                "tournamentStatus": result.tournament_status,
                "tournamentRevision": result.tournament_revision,
                "sourceRecruitmentId": result.source_recruitment_id,
                "sourceRecruitmentRevision": result.source_recruitment_revision,
                "reversedGameCount": result.reversed_game_count,
                "reason": command.reason.trim(),
            })),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.tournament_outbox_event_id,
        format!("guild:{guild_id}:auction-tournament:{tournament_id}"),
        "AUCTION_TOURNAMENT",
        &tournament_id_text,
        result.tournament_revision,
        "AUCTION_TOURNAMENT_CANCELLED",
        json!({"type":"invalidate","resourceId":tournament_id,"revision":result.tournament_revision}),
        prepared.now,
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.recruitment_outbox_event_id,
        format!(
            "guild:{guild_id}:auction-recruitment:{}",
            result.source_recruitment_id
        ),
        "AUCTION_RECRUITMENT",
        &recruitment_id_text,
        result.source_recruitment_revision,
        "AUCTION_RECRUITMENT_CANCELLED",
        json!({
            "type":"invalidate",
            "resourceId":result.source_recruitment_id,
            "revision":result.source_recruitment_revision
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn cancel_web_auction_tournament_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebAuctionTournamentCancelCommand,
    prepared: AuctionTournamentCancelPrepared,
) -> Result<AuctionTournamentCancelResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionTournamentCancelResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: AUCTION_TOURNAMENT_CANCEL_METHOD,
            route_template: AUCTION_TOURNAMENT_CANCEL_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let cancellation = cancel_locked_auction_tournament(
        transaction,
        guild_id,
        tournament_id,
        actor_id,
        request_id,
        prepared.now,
        Some(command.expected_revision),
        prepared.recruitment_notification_id,
    )
    .await?;
    let before = cancellation.before;
    let result = cancellation.result;
    let tournament_id_text = tournament_id.to_string();
    let recruitment_id_text = result.source_recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "AUCTION_TOURNAMENT_CANCELLED",
            target_type: "AUCTION_TOURNAMENT",
            target_id: &tournament_id_text,
            request_id,
            before_summary: Some(before),
            after_summary: Some(json!({
                "tournamentStatus": result.tournament_status,
                "tournamentRevision": result.tournament_revision,
                "sourceRecruitmentId": result.source_recruitment_id,
                "sourceRecruitmentRevision": result.source_recruitment_revision,
                "reversedGameCount": result.reversed_game_count,
                "reason": command.reason.trim(),
            })),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.tournament_outbox_event_id,
        format!("guild:{guild_id}:auction-tournament:{tournament_id}"),
        "AUCTION_TOURNAMENT",
        &tournament_id_text,
        result.tournament_revision,
        "AUCTION_TOURNAMENT_CANCELLED",
        json!({"type":"invalidate","resourceId":tournament_id,"revision":result.tournament_revision}),
        prepared.now,
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.recruitment_outbox_event_id,
        format!(
            "guild:{guild_id}:auction-recruitment:{}",
            result.source_recruitment_id
        ),
        "AUCTION_RECRUITMENT",
        &recruitment_id_text,
        result.source_recruitment_revision,
        "AUCTION_RECRUITMENT_CANCELLED",
        json!({
            "type":"invalidate",
            "resourceId":result.source_recruitment_id,
            "revision":result.source_recruitment_revision
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;

    Ok(result)
}

struct AuctionRecruitmentForceCancelOutcome {
    result: AuctionRecruitmentForceCancelResult,
    before: Value,
}

#[allow(clippy::too_many_arguments)]
async fn force_cancel_auction_recruitment_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    idempotency_key: &DiscordInteractionId,
    request_id: &str,
    request_hash: &[u8],
    command: &AuctionRecruitmentForceCancelCommand,
    prepared: AuctionRecruitmentForceCancelPrepared,
) -> Result<AuctionRecruitmentForceCancelResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionRecruitmentForceCancelResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
            method: AUCTION_RECRUITMENT_FORCE_CANCEL_METHOD,
            route_template: AUCTION_RECRUITMENT_FORCE_CANCEL_ROUTE,
            key: idempotency_key.as_str(),
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let actor = &command.actor;
    let actor_id = UserId::from(&actor.id);
    sync_guild_actor(transaction, guild_id, actor, prepared.now).await?;
    require_active_member(transaction, guild_id, &actor.id).await?;
    require_operator(actor, &operator_role_id)?;

    let recruitment = lock_auction_recruitment(transaction, guild_id, recruitment_id).await?;
    let outcome = match recruitment.status {
        RecruitmentStatus::Open | RecruitmentStatus::Closed => {
            let before = serde_json::to_value(
                auction_recruitment_snapshot(transaction, guild_id, recruitment_id, &actor_id)
                    .await?,
            )?;
            let revision = sqlx::query_scalar::<_, i64>(
                "UPDATE auction_recruitments
                 SET status = 'CANCELLED',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     cancelled_at = $2
                 WHERE guild_id = $3
                   AND id = $4
                   AND status IN ('OPEN', 'CLOSED')
                 RETURNING revision",
            )
            .bind(&actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AttemptError::RecruitmentNotOpen(recruitment.status))?;
            let snapshot =
                auction_recruitment_snapshot(transaction, guild_id, recruitment_id, &actor_id)
                    .await?;
            if snapshot.recruitment.status != RecruitmentStatus::Cancelled
                || snapshot.recruitment.revision != revision
            {
                return Err(AttemptError::InvariantViolation);
            }
            if let (Some(channel_id), Some(_message_id)) = (
                snapshot.recruitment.channel_id.as_ref(),
                snapshot.recruitment.message_id.as_ref(),
            ) {
                enqueue_auction_recruitment_message_notification(
                    transaction,
                    guild_id,
                    channel_id,
                    &snapshot,
                    AUCTION_RECRUITMENT_MESSAGE_DELETE_TYPE,
                    format!(
                        "auction-recruitment:{}:message:delete",
                        snapshot.recruitment.id
                    ),
                    prepared.recruitment_notification_id,
                    prepared.now,
                )
                .await?;
            }
            AuctionRecruitmentForceCancelOutcome {
                result: AuctionRecruitmentForceCancelResult {
                    recruitment_id,
                    recruitment_status: RecruitmentStatus::Cancelled,
                    recruitment_revision: revision,
                    tournament_id: None,
                    tournament_status: None,
                    tournament_revision: None,
                    reversed_game_count: 0,
                },
                before,
            }
        }
        RecruitmentStatus::Converted => {
            let tournament_id =
                auction_tournament_id_for_source_recruitment(transaction, guild_id, recruitment_id)
                    .await?;
            let cancellation = cancel_locked_auction_tournament(
                transaction,
                guild_id,
                tournament_id,
                &actor_id,
                request_id,
                prepared.now,
                None,
                prepared.recruitment_notification_id,
            )
            .await?;
            if cancellation.result.source_recruitment_id != recruitment_id {
                return Err(AttemptError::InvariantViolation);
            }
            AuctionRecruitmentForceCancelOutcome {
                result: AuctionRecruitmentForceCancelResult {
                    recruitment_id,
                    recruitment_status: RecruitmentStatus::Cancelled,
                    recruitment_revision: cancellation.result.source_recruitment_revision,
                    tournament_id: Some(cancellation.result.tournament_id),
                    tournament_status: Some(cancellation.result.tournament_status),
                    tournament_revision: Some(cancellation.result.tournament_revision),
                    reversed_game_count: cancellation.result.reversed_game_count,
                },
                before: cancellation.before,
            }
        }
        status => return Err(AttemptError::RecruitmentNotOpen(status)),
    };

    let result = outcome.result;
    let recruitment_id_text = recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "AUCTION_RECRUITMENT_FORCE_CANCELLED",
            target_type: "AUCTION_RECRUITMENT",
            target_id: &recruitment_id_text,
            request_id,
            before_summary: Some(outcome.before),
            after_summary: Some(json!({
                "recruitmentStatus": result.recruitment_status,
                "recruitmentRevision": result.recruitment_revision,
                "tournamentId": result.tournament_id,
                "tournamentStatus": result.tournament_status,
                "tournamentRevision": result.tournament_revision,
                "reversedGameCount": result.reversed_game_count,
                "source": "DISCORD_FORCE_CANCEL",
            })),
            now: prepared.now,
        },
    )
    .await?;
    if let (Some(tournament_id), Some(tournament_revision)) =
        (result.tournament_id, result.tournament_revision)
    {
        let tournament_id_text = tournament_id.to_string();
        insert_outbox(
            transaction,
            guild_id,
            prepared.tournament_outbox_event_id,
            format!("guild:{guild_id}:auction-tournament:{tournament_id}"),
            "AUCTION_TOURNAMENT",
            &tournament_id_text,
            tournament_revision,
            "AUCTION_TOURNAMENT_CANCELLED",
            json!({"type":"invalidate","resourceId":tournament_id,"revision":tournament_revision}),
            prepared.now,
        )
        .await?;
    }
    insert_outbox(
        transaction,
        guild_id,
        prepared.recruitment_outbox_event_id,
        format!("guild:{guild_id}:auction-recruitment:{recruitment_id}"),
        "AUCTION_RECRUITMENT",
        &recruitment_id_text,
        result.recruitment_revision,
        "AUCTION_RECRUITMENT_CANCELLED",
        json!({
            "type":"invalidate",
            "resourceId":recruitment_id,
            "revision":result.recruitment_revision
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;

    Ok(result)
}

struct AuctionTournamentCancellationOutcome {
    result: AuctionTournamentCancelResult,
    before: Value,
}

#[allow(clippy::too_many_arguments)]
async fn cancel_locked_auction_tournament(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    request_id: &str,
    now: OffsetDateTime,
    expected_revision: Option<i64>,
    recruitment_notification_id: NotificationId,
) -> Result<AuctionTournamentCancellationOutcome, AttemptError> {
    // Tournament -> source recruitment -> matches -> games is the shared lifecycle lock order.
    let tournament =
        lock_auction_tournament_for_cancel(transaction, guild_id, tournament_id).await?;
    if let Some(expected_revision) = expected_revision
        && tournament.revision != expected_revision
    {
        return Err(AttemptError::AuctionTournamentRevisionConflict {
            expected: expected_revision,
            actual: tournament.revision,
        });
    }
    if !matches!(
        tournament.status,
        AuctionTournamentStatus::Bidding
            | AuctionTournamentStatus::BracketSetup
            | AuctionTournamentStatus::InGame
    ) {
        return Err(AttemptError::AuctionTournamentCancelUnavailable(
            tournament.status,
        ));
    }
    let source_recruitment =
        lock_auction_recruitment(transaction, guild_id, tournament.source_recruitment_id).await?;
    if source_recruitment.status != RecruitmentStatus::Converted {
        return Err(AttemptError::AuctionTournamentSourceNotReady(
            source_recruitment.status,
        ));
    }
    let _active_matches =
        lock_auction_tournament_matches(transaction, guild_id, tournament_id).await?;

    let before = json!({
        "tournamentStatus": tournament.status,
        "tournamentRevision": tournament.revision,
        "sourceRecruitmentId": tournament.source_recruitment_id,
        "sourceRecruitmentStatus": source_recruitment.status,
    });
    let mut reversed_game_count = 0_i64;
    while confirmed_auction_tournament_game_count(transaction, guild_id, tournament_id).await? > 0 {
        void_last_confirmed_auction_tournament_game(
            transaction,
            guild_id,
            tournament_id,
            actor_id,
            request_id,
            now,
        )
        .await?;
        reversed_game_count = reversed_game_count
            .checked_add(1)
            .ok_or(AttemptError::InvariantViolation)?;
    }

    cancel_active_auction_tournament_matches(transaction, guild_id, tournament_id, actor_id, now)
        .await?;
    let tournament_revision =
        cancel_auction_tournament_state(transaction, guild_id, tournament_id, actor_id, now)
            .await?;
    let source_recruitment_revision = cancel_auction_tournament_source_recruitment(
        transaction,
        guild_id,
        tournament.source_recruitment_id,
        actor_id,
        now,
    )
    .await?;
    let source_snapshot = auction_recruitment_snapshot(
        transaction,
        guild_id,
        tournament.source_recruitment_id,
        actor_id,
    )
    .await?;
    if source_snapshot.recruitment.status != RecruitmentStatus::Cancelled
        || source_snapshot.recruitment.revision != source_recruitment_revision
    {
        return Err(AttemptError::InvariantViolation);
    }
    if let (Some(channel_id), Some(_message_id)) = (
        source_snapshot.recruitment.channel_id.as_ref(),
        source_snapshot.recruitment.message_id.as_ref(),
    ) {
        enqueue_auction_recruitment_message_notification(
            transaction,
            guild_id,
            channel_id,
            &source_snapshot,
            AUCTION_RECRUITMENT_MESSAGE_DELETE_TYPE,
            format!(
                "auction-recruitment:{}:message:delete",
                source_snapshot.recruitment.id
            ),
            recruitment_notification_id,
            now,
        )
        .await?;
    }

    Ok(AuctionTournamentCancellationOutcome {
        result: AuctionTournamentCancelResult {
            tournament_id,
            tournament_status: AuctionTournamentStatus::Cancelled,
            tournament_revision,
            source_recruitment_id: tournament.source_recruitment_id,
            source_recruitment_revision,
            reversed_game_count,
        },
        before,
    })
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_auction_match_lineup_mutation_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebAuctionMatchLineupMutation,
    prepared: WebAuctionMatchLineupPrepared,
) -> Result<AuctionMatchSnapshot, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionMatchSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: command.method(),
            route_template: command.route_template(),
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    // Match mutations share the tournament-first lock order used by bracket mutations.
    let tournament_id = auction_match_tournament_id(transaction, guild_id, match_id).await?;
    let tournament = lock_auction_tournament(transaction, guild_id, tournament_id).await?;
    if !matches!(
        tournament.status,
        AuctionTournamentStatus::BracketSetup | AuctionTournamentStatus::InGame
    ) {
        return Err(AttemptError::AuctionMatchTournamentNotReady);
    }
    let auction_match = lock_auction_match(transaction, guild_id, match_id).await?;
    if auction_match.tournament_id != tournament_id.as_uuid() {
        return Err(AttemptError::InvariantViolation);
    }
    if auction_match.revision != command.expected_revision() {
        return Err(AttemptError::AuctionMatchRevisionConflict {
            expected: command.expected_revision(),
            actual: auction_match.revision,
        });
    }
    let (left_team_id, right_team_id) = auction_match_lineup_teams(&auction_match)?;
    let _teams = lock_auction_tournament_teams(transaction, guild_id, tournament_id).await?;
    let members =
        lock_auction_tournament_team_members(transaction, guild_id, tournament_id).await?;
    let lineups =
        lock_auction_match_lineups(transaction, guild_id, tournament_id, match_id).await?;
    validate_auction_match_lineup_rows(&lineups, &members, left_team_id, right_team_id, false)?;
    let before = auction_match_snapshot(transaction, guild_id, match_id, actor_id).await?;

    let revision = match command {
        WebAuctionMatchLineupMutation::Assign(command) => {
            ensure_auction_match_lineup_editable(&auction_match)?;
            if command.team_id != left_team_id && command.team_id != right_team_id {
                return Err(AttemptError::AuctionMatchLineupTeamNotFound);
            }
            match command.team_member_id {
                Some(team_member_id) => {
                    let member = members
                        .iter()
                        .find(|member| {
                            member.team_id == command.team_id && member.id == team_member_id
                        })
                        .ok_or(AttemptError::AuctionMatchLineupMemberNotFound)?;
                    if lineups.iter().any(|lineup| {
                        lineup.user_id == member.user_id
                            && (lineup.team_id != command.team_id || lineup.lane != command.lane)
                    }) {
                        return Err(AttemptError::AuctionMatchLineupPlayerAssigned);
                    }
                    sqlx::query(
                        "INSERT INTO auction_match_lineups (
                             guild_id, tournament_id, match_id, team_id, lane, team_member_id, user_id
                         ) VALUES ($1, $2, $3, $4, $5, $6, $7)
                         ON CONFLICT (guild_id, tournament_id, match_id, team_id, lane)
                         DO UPDATE SET team_member_id = EXCLUDED.team_member_id,
                                       user_id = EXCLUDED.user_id",
                    )
                    .bind(guild_id)
                    .bind(tournament_id.as_uuid())
                    .bind(match_id.as_uuid())
                    .bind(command.team_id)
                    .bind(command.lane)
                    .bind(team_member_id)
                    .bind(&member.user_id)
                    .execute(&mut **transaction)
                    .await?;
                }
                None => {
                    sqlx::query(
                        "DELETE FROM auction_match_lineups
                         WHERE guild_id = $1
                           AND tournament_id = $2
                           AND match_id = $3
                           AND team_id = $4
                           AND lane = $5",
                    )
                    .bind(guild_id)
                    .bind(tournament_id.as_uuid())
                    .bind(match_id.as_uuid())
                    .bind(command.team_id)
                    .bind(command.lane)
                    .execute(&mut **transaction)
                    .await?;
                }
            }
            bump_auction_match_lineup_revision(
                transaction,
                guild_id,
                match_id,
                actor_id,
                prepared.now,
            )
            .await?
        }
        WebAuctionMatchLineupMutation::Lock(_) => {
            ensure_auction_match_lineup_editable(&auction_match)?;
            validate_auction_match_lineup_rows(
                &lineups,
                &members,
                left_team_id,
                right_team_id,
                true,
            )?;
            let revision =
                lock_auction_match_lineup(transaction, guild_id, match_id, actor_id, prepared.now)
                    .await?;
            advance_auction_tournament_for_lineup_lock(
                transaction,
                guild_id,
                tournament_id,
                actor_id,
                prepared.now,
            )
            .await?;
            revision
        }
        WebAuctionMatchLineupMutation::Unlock(_) => {
            ensure_auction_match_lineup_unlockable(&auction_match)?;
            validate_auction_match_lineup_rows(
                &lineups,
                &members,
                left_team_id,
                right_team_id,
                true,
            )?;
            ensure_auction_match_has_no_result_artifacts(transaction, guild_id, match_id).await?;
            let revision = unlock_auction_match_lineup(
                transaction,
                guild_id,
                match_id,
                actor_id,
                prepared.now,
            )
            .await?;
            downgrade_auction_tournament_after_lineup_unlock(
                transaction,
                guild_id,
                tournament_id,
                actor_id,
                prepared.now,
            )
            .await?;
            revision
        }
    };
    let snapshot = auction_match_snapshot(transaction, guild_id, match_id, actor_id).await?;
    if snapshot.r#match.revision != revision {
        return Err(AttemptError::InvariantViolation);
    }
    if matches!(command, WebAuctionMatchLineupMutation::Lock(_)) {
        if let Some(channel_id) = configured_entry_channel(transaction, guild_id).await? {
            let confirmation_snapshot =
                auction_match_lineup_confirmation_snapshot(guild_id, &tournament, &snapshot)?;
            enqueue_auction_match_lineup_confirmation_notification(
                transaction,
                guild_id,
                &channel_id,
                &confirmation_snapshot,
                revision,
                prepared.lineup_confirmation_notification_id,
                prepared.now,
            )
            .await?;
        }
    }
    let target_id = match_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: command.audit_action(),
            target_type: "AUCTION_MATCH",
            target_id: &target_id,
            request_id,
            before_summary: Some(serde_json::to_value(&before)?),
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:auction-match:{match_id}"),
        "AUCTION_MATCH",
        &target_id,
        revision,
        "AUCTION_MATCH_LINEUP_UPDATED",
        json!({"type":"invalidate","resourceId":match_id,"revision":revision}),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_auction_match_game_mutation_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebAuctionMatchGameMutation,
    prepared: WebAuctionMatchGameMutationPrepared,
) -> Result<AuctionMatchSnapshot, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<AuctionMatchSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: command.method(),
            route_template: command.route_template(),
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let tournament_id = auction_match_tournament_id(transaction, guild_id, match_id).await?;
    let tournament = lock_auction_tournament(transaction, guild_id, tournament_id).await?;
    let tournament_allows_mutation = match command {
        WebAuctionMatchGameMutation::Submit(_) => {
            tournament.status == AuctionTournamentStatus::InGame
        }
        WebAuctionMatchGameMutation::Revert(_) => matches!(
            tournament.status,
            AuctionTournamentStatus::InGame | AuctionTournamentStatus::Completed
        ),
    };
    if !tournament_allows_mutation {
        return Err(AttemptError::AuctionMatchTournamentNotReady);
    }
    let auction_match = lock_auction_match(transaction, guild_id, match_id).await?;
    if auction_match.tournament_id != tournament_id.as_uuid() {
        return Err(AttemptError::InvariantViolation);
    }
    if auction_match.revision != command.expected_revision() {
        return Err(AttemptError::AuctionMatchRevisionConflict {
            expected: command.expected_revision(),
            actual: auction_match.revision,
        });
    }
    let (left_team_id, right_team_id) = auction_match_lineup_teams(&auction_match)?;
    if auction_match.lineups_locked_at.is_none() {
        return Err(AttemptError::AuctionMatchLineupIncomplete);
    }
    if matches!(command, WebAuctionMatchGameMutation::Submit(_)) {
        ensure_twenty_person_semifinal_lineups_locked(
            transaction,
            guild_id,
            tournament_id,
            &tournament.format,
            &auction_match,
        )
        .await?;
    }
    ensure_auction_match_result_editable(&auction_match, command)?;
    let members =
        lock_auction_tournament_team_members(transaction, guild_id, tournament_id).await?;
    let lineups =
        lock_auction_match_lineups(transaction, guild_id, tournament_id, match_id).await?;
    validate_auction_match_lineup_rows(&lineups, &members, left_team_id, right_team_id, true)?;
    let roster = auction_match_roster_from_lineups(&lineups, left_team_id, right_team_id)?;
    let before = auction_match_snapshot(transaction, guild_id, match_id, actor_id).await?;

    let (revision, completed_tournament_revision) = match command {
        WebAuctionMatchGameMutation::Submit(command) => {
            let confirmed_game_count =
                confirmed_auction_match_game_count(transaction, guild_id, match_id).await?;
            let expected_set_number =
                next_auction_match_set_number(confirmed_game_count, auction_match.best_of)?;
            if command.set_number != expected_set_number {
                return Err(AttemptError::AuctionMatchSetNotCurrent {
                    expected: expected_set_number,
                    actual: command.set_number,
                });
            }
            let input_id = ensure_locked_auction_match_draft_input(
                transaction,
                guild_id,
                match_id,
                expected_set_number,
                prepared.input_id,
                actor_id,
                prepared.now,
            )
            .await?;
            let submit = SeriesGameSubmitCommand {
                set_number: command.set_number,
                expected_revision: command.expected_revision,
                side_preset: command.side_preset.clone(),
                winner: command.winner.clone(),
                picks: Some(command.picks.clone()),
                bans: Some(command.bans.clone()),
                patch_version: command.patch_version.clone(),
            };
            replace_series_draft_from_submit(transaction, guild_id, input_id, &roster, &submit)
                .await?;
            validate_auction_match_pick_ban_draft(
                transaction,
                guild_id,
                match_id,
                auction_match.hard_fearless,
            )
            .await?;
            let input = sqlx::query_as::<_, SeriesPickBanGameInputRow>(
                "SELECT id, set_number, side_preset, side_selected, winning_side, patch_version
                 FROM game_inputs
                 WHERE guild_id = $1 AND id = $2 AND status = 'DRAFT'
                 FOR UPDATE",
            )
            .bind(guild_id)
            .bind(input_id)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AttemptError::InvariantViolation)?;
            let winner = input
                .winning_side
                .as_deref()
                .map(canonical_blue_red_side)
                .transpose()?
                .ok_or(AttemptError::InvalidGameResult)?;
            let side_preset = canonical_side_preset(&input.side_preset)?;
            let winner_team = winner_roster_team(&side_preset, &winner)?;
            let picks = series_input_picks(transaction, guild_id, input_id, &roster).await?;
            let bans = series_input_bans(transaction, guild_id, input_id).await?;
            let season_id = SeasonId::from_uuid(auction_match.season_id);
            sqlx::query(
                "INSERT INTO games (
                     guild_id, id, season_id, series_id, auction_match_id, source_input_id, game_number,
                     side_preset, winning_side, duration_seconds, patch_version, riot_match_id, status,
                     mmr_status, played_at, confirmed_by, confirmed_by_type, confirmed_at,
                     voided_by, voided_by_type, voided_at, void_reason
                 ) VALUES (
                     $1, $2, $3, NULL, $4, $5, $6,
                     $7, $8, NULL, $9, NULL, 'CONFIRMED', 'APPLIED', $10, $11, 'USER', $10,
                     NULL, NULL, NULL, NULL
                 )",
            )
            .bind(guild_id)
            .bind(prepared.game_id.as_uuid())
            .bind(season_id.as_uuid())
            .bind(match_id.as_uuid())
            .bind(input_id)
            .bind(input.set_number)
            .bind(&side_preset)
            .bind(&winner)
            .bind(&input.patch_version)
            .bind(prepared.now)
            .bind(actor_id)
            .execute(&mut **transaction)
            .await?;
            insert_series_game_participants(
                transaction,
                guild_id,
                prepared.game_id,
                &roster,
                &picks,
            )
            .await?;
            insert_series_game_bans(transaction, guild_id, prepared.game_id, &bans).await?;
            apply_series_game_mmr(
                transaction,
                guild_id,
                season_id,
                prepared.game_id,
                &roster,
                winner_team,
                actor_id,
                request_id,
                prepared.now,
            )
            .await?;
            let submitted = sqlx::query(
                "UPDATE game_inputs
                 SET status = 'SUBMITTED', updated_by = $1, updated_at = $2
                 WHERE guild_id = $3 AND id = $4 AND status = 'DRAFT'",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(input_id)
            .execute(&mut **transaction)
            .await?;
            if submitted.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
            let (left_score, right_score) =
                confirmed_auction_match_score(transaction, guild_id, match_id).await?;
            let required_wins = i64::from(auction_match.best_of / 2 + 1);
            let winner_team_id = if left_score >= required_wins {
                Some(left_team_id)
            } else if right_score >= required_wins {
                Some(right_team_id)
            } else {
                None
            };
            let revision = update_auction_match_result_state(
                transaction,
                guild_id,
                match_id,
                actor_id,
                prepared.now,
                winner_team_id,
            )
            .await?;
            let completed_tournament_revision = match winner_team_id {
                Some(winner_team_id) => {
                    advance_auction_tournament_after_match_completion(
                        transaction,
                        guild_id,
                        tournament_id,
                        match_id,
                        &auction_match,
                        winner_team_id,
                        actor_id,
                        prepared.now,
                    )
                    .await?
                }
                None => None,
            };
            (revision, completed_tournament_revision)
        }
        WebAuctionMatchGameMutation::Revert(_) => {
            let was_completed = auction_match.status == "COMPLETED";
            if was_completed && auction_match.match_key == "FINAL" {
                reconcile_auction_tournament_result_notifications_after_revert(
                    transaction,
                    guild_id,
                    tournament_id,
                    prepared.now,
                )
                .await?;
            }
            let _reverted = void_last_confirmed_auction_match_game(
                transaction,
                guild_id,
                match_id,
                actor_id,
                request_id,
                prepared.now,
            )
            .await?;
            let confirmed_game_count =
                confirmed_auction_match_game_count(transaction, guild_id, match_id).await?;
            let revision = reopen_auction_match_after_game_revert(
                transaction,
                guild_id,
                match_id,
                actor_id,
                prepared.now,
                confirmed_game_count,
            )
            .await?;
            if was_completed {
                reopen_auction_tournament_after_match_revert(
                    transaction,
                    guild_id,
                    tournament_id,
                    match_id,
                    &auction_match,
                    actor_id,
                    prepared.now,
                )
                .await?;
            }
            (revision, None)
        }
    };

    if let Some(tournament_revision) = completed_tournament_revision {
        enqueue_auction_tournament_result_notification_if_configured(
            transaction,
            guild_id,
            tournament_id,
            tournament_revision,
            prepared.result_notification_id,
            prepared.now,
        )
        .await?;
    }
    let snapshot = auction_match_snapshot(transaction, guild_id, match_id, actor_id).await?;
    if snapshot.r#match.revision != revision {
        return Err(AttemptError::InvariantViolation);
    }
    let target_id = match_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: command.audit_action(),
            target_type: "AUCTION_MATCH_GAME",
            target_id: &target_id,
            request_id,
            before_summary: Some(serde_json::to_value(&before)?),
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:auction-match:{match_id}"),
        "AUCTION_MATCH",
        &target_id,
        revision,
        command.event_type(),
        json!({"type":"invalidate","resourceId":match_id,"revision":revision}),
        prepared.now,
    )
    .await?;
    let status = match command {
        WebAuctionMatchGameMutation::Submit(_) => 201,
        WebAuctionMatchGameMutation::Revert(_) => 200,
    };
    complete_idempotency(
        transaction,
        &scope,
        record_id,
        status,
        &snapshot,
        prepared.now,
    )
    .await?;
    Ok(snapshot)
}

async fn insert_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    rules: RecruitmentSeriesRules,
    expected_start_at: OffsetDateTime,
    recruitment_id: RecruitmentId,
    now: OffsetDateTime,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let season_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .map(SeasonId::from_uuid)
    .ok_or(AttemptError::NoActiveSeason)?;

    let display_number = sqlx::query_scalar::<_, i64>(
        "UPDATE guild_counters
         SET next_value = next_value + 1
         WHERE guild_id = $1 AND counter_key = 'RECRUITMENT'
         RETURNING next_value - 1",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingRecruitmentCounter)?;
    let title = recruitment_title(display_number, expected_start_at)?;

    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, best_of, hard_fearless,
             status, revision, channel_id, message_id, created_by, created_at, updated_by, updated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'OPEN', 0, NULL, NULL, $9, $10, $9, $10)",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(season_id.as_uuid())
    .bind(display_number)
    .bind(title)
    .bind(RECRUITMENT_TARGET_COUNT)
    .bind(rules.best_of)
    .bind(rules.hard_fearless)
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO recruitment_entry_drafts (
             guild_id, recruitment_id, revision, updated_by, updated_at
         ) VALUES ($1, $2, 0, $3, $4)",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;

    recruitment_snapshot(transaction, guild_id, recruitment_id).await
}

async fn insert_auction_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    format: AuctionFormat,
    rules: AuctionRecruitmentSeriesRules,
    expected_start_at: OffsetDateTime,
    recruitment_id: AuctionRecruitmentId,
    now: OffsetDateTime,
) -> Result<AuctionRecruitmentSnapshot, AttemptError> {
    let season_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .map(SeasonId::from_uuid)
    .ok_or(AttemptError::NoActiveSeason)?;
    let display_number = sqlx::query_scalar::<_, i64>(
        "UPDATE guild_counters
         SET next_value = next_value + 1
         WHERE guild_id = $1 AND counter_key = 'AUCTION_RECRUITMENT'
         RETURNING next_value - 1",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingAuctionRecruitmentCounter)?;
    let title = auction_recruitment_title(format, display_number, expected_start_at)?;

    sqlx::query(
        "INSERT INTO auction_recruitments (
             guild_id, id, season_id, display_number, title, format, target_count,
             semifinal_best_of, final_best_of, hard_fearless, status, revision, channel_id,
             message_id, created_by, created_at, updated_by, updated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'OPEN', 0, NULL, NULL, $11, $12, $11, $12)",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(season_id.as_uuid())
    .bind(display_number)
    .bind(title)
    .bind(format.as_str())
    .bind(format.participant_count())
    .bind(rules.semifinal_best_of)
    .bind(rules.final_best_of)
    .bind(rules.hard_fearless)
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO auction_setup_drafts (
             guild_id, recruitment_id, revision, updated_by, updated_at
         ) VALUES ($1, $2, 0, $3, $4)",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    for team_index in 1..=format.team_count() {
        sqlx::query(
            "INSERT INTO auction_setup_teams (
                 guild_id, recruitment_id, team_index, team_name, captain_user_id, initial_points
             ) VALUES ($1, $2, $3, $4, NULL, $5)",
        )
        .bind(guild_id)
        .bind(recruitment_id.as_uuid())
        .bind(team_index)
        .bind(format!("팀 {team_index}"))
        .bind(AUCTION_INITIAL_TEAM_POINTS)
        .execute(&mut **transaction)
        .await?;
    }

    auction_recruitment_snapshot(transaction, guild_id, recruitment_id, actor_id).await
}

fn expected_start_label(expected_start_at: OffsetDateTime) -> Result<String, AttemptError> {
    let korea_standard_time =
        UtcOffset::from_hms(9, 0, 0).map_err(|_| AttemptError::InvariantViolation)?;
    let expected_start = expected_start_at.to_offset(korea_standard_time);
    Ok(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        expected_start.year(),
        u8::from(expected_start.month()),
        expected_start.day(),
        expected_start.hour(),
        expected_start.minute(),
    ))
}

fn recruitment_title(
    display_number: i64,
    expected_start_at: OffsetDateTime,
) -> Result<String, AttemptError> {
    let expected_start = expected_start_label(expected_start_at)?;
    Ok(format!(
        "일반 내전 모집 #{display_number} · 예상 시작 {expected_start} (KST)"
    ))
}

fn auction_recruitment_title(
    format: AuctionFormat,
    display_number: i64,
    expected_start_at: OffsetDateTime,
) -> Result<String, AttemptError> {
    let expected_start = expected_start_label(expected_start_at)?;
    Ok(format!(
        "{}인 경매 내전 모집 #{display_number} · 예상 시작 {expected_start} (KST)",
        format.participant_count()
    ))
}

async fn auction_recruitment_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    viewer_id: &UserId,
) -> Result<AuctionRecruitmentSnapshot, AttemptError> {
    let recruitment = sqlx::query_as::<_, AuctionRecruitmentProjectionRow>(
        "SELECT recruitment.display_number,
                recruitment.title,
                recruitment.format,
                recruitment.target_count,
                recruitment.semifinal_best_of,
                recruitment.final_best_of,
                recruitment.hard_fearless,
                recruitment.status,
                recruitment.revision,
                recruitment.channel_id,
                recruitment.message_id,
                recruitment.created_by,
                recruitment.updated_at,
                count(participant.user_id) FILTER (WHERE participant.status = 'JOINED')
                    AS joined_participant_count
         FROM auction_recruitments AS recruitment
         LEFT JOIN auction_recruitment_participants AS participant
           ON participant.guild_id = recruitment.guild_id
          AND participant.recruitment_id = recruitment.id
         WHERE recruitment.guild_id = $1 AND recruitment.id = $2
         GROUP BY recruitment.id, recruitment.display_number, recruitment.title, recruitment.format,
                  recruitment.target_count, recruitment.semifinal_best_of,
                  recruitment.final_best_of, recruitment.hard_fearless, recruitment.status, recruitment.revision,
                  recruitment.channel_id, recruitment.message_id, recruitment.created_by,
                  recruitment.updated_at",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionRecruitmentNotFound)?;
    let format = parse_auction_format(&recruitment.format)?;
    if recruitment.target_count != format.participant_count() {
        return Err(AttemptError::InvariantViolation);
    }
    let rules = AuctionRecruitmentSeriesRules {
        hard_fearless: recruitment.hard_fearless,
        semifinal_best_of: recruitment.semifinal_best_of,
        final_best_of: recruitment.final_best_of,
    };
    validate_auction_recruitment_series_rules(format, &rules)
        .map_err(|_| AttemptError::InvariantViolation)?;
    let participant_rows = sqlx::query_as::<_, AuctionRecruitmentParticipantRow>(
        "SELECT participant.user_id AS id, member.display_name, participant.declared_lane
         FROM auction_recruitment_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         WHERE participant.guild_id = $1
           AND participant.recruitment_id = $2
           AND participant.status = 'JOINED'
         ORDER BY participant.joined_at, participant.user_id",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let is_operator = is_active_session_operator(transaction, guild_id, viewer_id).await?;
    let status = recruitment.status;
    let created_by = recruitment.created_by.clone();
    let draft = (status == RecruitmentStatus::Closed).then(|| {
        format!(
            "/g/{}/auction/recruitments/{}/draft",
            guild_id.as_str(),
            recruitment_id
        )
    });

    Ok(AuctionRecruitmentSnapshot {
        recruitment: AuctionRecruitmentSummarySnapshot {
            id: recruitment_id,
            display_number: recruitment.display_number,
            title: recruitment.title,
            format,
            rules,
            participant_count: recruitment.target_count,
            joined_participant_count: recruitment.joined_participant_count,
            team_count: format.team_count(),
            status,
            revision: recruitment.revision,
            channel_id: recruitment.channel_id,
            message_id: recruitment.message_id,
            created_by_id: created_by.clone(),
            updated_at: recruitment.updated_at,
            href: format!(
                "/g/{}/auction/recruitments/{recruitment_id}",
                guild_id.as_str()
            ),
        },
        participants: participant_rows
            .into_iter()
            .map(|row| AuctionRecruitmentParticipantSnapshot {
                id: row.id,
                display_name: row.display_name,
                declared_lane: row.declared_lane,
            })
            .collect(),
        links: AuctionRecruitmentLinksSnapshot { draft },
        permissions: AuctionRecruitmentPermissions {
            can_manage_members: status == RecruitmentStatus::Open && is_operator,
            can_close: status == RecruitmentStatus::Open && is_operator,
            can_send_participant_confirmation_dms: status == RecruitmentStatus::Closed
                && is_operator,
            can_cancel: status == RecruitmentStatus::Open
                && (is_operator || created_by == *viewer_id),
        },
    })
}

#[derive(Clone)]
struct ConfiguredAuctionSetupTeam {
    team_index: i16,
    team_name: String,
    captain_user_id: UserId,
    initial_points: i32,
}

async fn auction_setup_draft_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    viewer_id: &UserId,
) -> Result<AuctionSetupDraftSnapshot, AttemptError> {
    require_active_session_member_read(transaction, guild_id, viewer_id).await?;
    let recruitment = sqlx::query_as::<_, AuctionRecruitmentLockRow>(
        "SELECT season_id, format, status, target_count, semifinal_best_of, final_best_of,
                hard_fearless, revision, created_by
         FROM auction_recruitments
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionRecruitmentNotFound)?;
    ensure_auction_setup_editable(recruitment.status)?;
    let format = parse_auction_format(&recruitment.format)?;
    if recruitment.target_count != format.participant_count() {
        return Err(AttemptError::InvariantViolation);
    }
    let draft = sqlx::query_as::<_, AuctionSetupDraftRow>(
        "SELECT revision
         FROM auction_setup_drafts
         WHERE guild_id = $1 AND recruitment_id = $2",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingAuctionSetupDraft)?;
    let teams = sqlx::query_as::<_, AuctionSetupTeamRow>(
        "SELECT team_index, team_name, captain_user_id, initial_points
         FROM auction_setup_teams
         WHERE guild_id = $1 AND recruitment_id = $2
         ORDER BY team_index",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let participants = sqlx::query_as::<_, AuctionRecruitmentParticipantRow>(
        "SELECT participant.user_id AS id, member.display_name, participant.declared_lane
         FROM auction_recruitment_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         WHERE participant.guild_id = $1
           AND participant.recruitment_id = $2
           AND participant.status = 'JOINED'
         ORDER BY participant.joined_at, participant.user_id",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let is_operator = is_active_session_operator(transaction, guild_id, viewer_id).await?;
    let can_create_tournament = is_operator
        && has_complete_auction_setup(
            &teams,
            format.team_count(),
            recruitment.target_count,
            &participants,
        );

    Ok(AuctionSetupDraftSnapshot {
        draft: AuctionSetupDraftStateSnapshot {
            recruitment_id,
            revision: draft.revision,
            team_count: format.team_count(),
            teams: teams
                .into_iter()
                .map(|team| AuctionSetupDraftTeamSnapshot {
                    team_index: team.team_index,
                    team_name: team.team_name,
                    captain_user_id: team.captain_user_id,
                    initial_points: team.initial_points,
                })
                .collect(),
        },
        participants: participants
            .into_iter()
            .map(|participant| AuctionRecruitmentParticipantSnapshot {
                id: participant.id,
                display_name: participant.display_name,
                declared_lane: participant.declared_lane,
            })
            .collect(),
        permissions: AuctionSetupDraftPermissions {
            can_edit: is_operator,
            can_create_tournament,
        },
    })
}

async fn auction_tournament_draft_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    viewer_id: &UserId,
) -> Result<AuctionTournamentDraftSnapshot, AttemptError> {
    require_active_session_member_read(transaction, guild_id, viewer_id).await?;
    let tournament = sqlx::query_as::<_, AuctionTournamentDraftRow>(
        "SELECT id, season_id, source_recruitment_id, display_number, title, format,
                semifinal_best_of, final_best_of, hard_fearless, status, revision,
                bracket_locked_at
         FROM auction_tournaments
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)?;
    let format = parse_auction_format(&tournament.format)?;
    let rules = AuctionRecruitmentSeriesRules {
        semifinal_best_of: tournament.semifinal_best_of,
        final_best_of: tournament.final_best_of,
        hard_fearless: tournament.hard_fearless,
    };
    validate_auction_recruitment_series_rules(format, &rules)
        .map_err(|_| AttemptError::InvariantViolation)?;
    let teams = sqlx::query_as::<_, AuctionTournamentTeamRow>(
        "SELECT id, team_index, team_name, initial_points, current_points, point_revision
         FROM auction_teams
         WHERE guild_id = $1 AND tournament_id = $2
         ORDER BY team_index",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let member_rows = sqlx::query_as::<_, AuctionTournamentTeamMemberRow>(
        "SELECT member.team_id, member.id, member.user_id, guild_member.display_name,
                member.declared_lane, member.mmr_at_acquisition, member.snapshot_origin,
                member.acquired_via, member.roster_slot
         FROM auction_team_members AS member
         JOIN guild_members AS guild_member
           ON guild_member.guild_id = member.guild_id
          AND guild_member.user_id = member.user_id
         WHERE member.guild_id = $1
           AND member.tournament_id = $2
           AND member.released_at IS NULL
         ORDER BY member.team_id, member.roster_slot",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let candidate_rows = sqlx::query_as::<_, AuctionTournamentCandidateRow>(
        "SELECT candidate.user_id, member.display_name, candidate.declared_lane,
                candidate.mmr_at_start, candidate.snapshot_origin, candidate.queue_position,
                candidate.state, candidate.draw_count
         FROM auction_bid_candidates AS candidate
         JOIN guild_members AS member
           ON member.guild_id = candidate.guild_id
          AND member.user_id = candidate.user_id
         WHERE candidate.guild_id = $1 AND candidate.tournament_id = $2
         ORDER BY candidate.queue_position, candidate.user_id",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let mut members_by_team = HashMap::<Uuid, Vec<AuctionTournamentTeamMemberSnapshot>>::new();
    for member in member_rows {
        members_by_team.entry(member.team_id).or_default().push(
            AuctionTournamentTeamMemberSnapshot {
                team_member_id: member.id,
                user_id: member.user_id,
                display_name: member.display_name,
                declared_lane: member.declared_lane,
                mmr_at_acquisition: member.mmr_at_acquisition,
                snapshot_origin: member.snapshot_origin,
                acquired_via: member.acquired_via,
                roster_slot: member.roster_slot,
            },
        );
    }

    let candidates = candidate_rows
        .into_iter()
        .map(|candidate| AuctionTournamentCandidateSnapshot {
            user_id: candidate.user_id,
            display_name: candidate.display_name,
            declared_lane: candidate.declared_lane,
            mmr_at_start: candidate.mmr_at_start,
            snapshot_origin: candidate.snapshot_origin,
            queue_position: candidate.queue_position,
            state: candidate.state,
            draw_count: candidate.draw_count,
        })
        .collect::<Vec<_>>();
    let active_candidates = candidates
        .iter()
        .filter(|candidate| candidate.state == "ACTIVE")
        .cloned()
        .collect::<Vec<_>>();
    if active_candidates.len() > 1 {
        return Err(AttemptError::InvariantViolation);
    }
    let active_lot_targets = sqlx::query_scalar::<_, UserId>(
        "SELECT target_user_id
         FROM auction_lots
         WHERE guild_id = $1 AND tournament_id = $2 AND status = 'ACTIVE'
         ORDER BY draw_sequence, id",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    if active_lot_targets.len() > 1
        || active_lot_targets.first().map(UserId::as_str)
            != active_candidates
                .first()
                .map(|candidate| candidate.user_id.as_str())
    {
        return Err(AttemptError::InvariantViolation);
    }
    let recent_action =
        auction_tournament_recent_unreversed_draft_action(transaction, guild_id, tournament_id)
            .await?;
    let is_operator = is_active_session_operator(transaction, guild_id, viewer_id).await?;
    let has_bracket_artifacts =
        auction_tournament_has_bracket_artifacts(transaction, guild_id, tournament_id).await?;
    let has_latest_revertible_draft_action =
        auction_tournament_has_latest_revertible_draft_action(transaction, guild_id, tournament_id)
            .await?;
    let unsold = candidates
        .iter()
        .filter(|candidate| candidate.state == "UNSOLD")
        .cloned()
        .collect();
    let current_target = active_candidates.into_iter().next();
    let current_target_profile = match current_target.as_ref() {
        Some(target) => Some(
            auction_tournament_current_target_profile(
                transaction,
                guild_id,
                SeasonId::from_uuid(tournament.season_id),
                target,
            )
            .await?,
        ),
        None => None,
    };
    let tournament_status = tournament.status;
    let can_revert_last = is_operator
        && has_latest_revertible_draft_action
        && !has_bracket_artifacts
        && (tournament_status == AuctionTournamentStatus::Bidding
            || (tournament_status == AuctionTournamentStatus::BracketSetup
                && tournament.bracket_locked_at.is_none()));

    Ok(AuctionTournamentDraftSnapshot {
        tournament: AuctionTournamentDraftSummarySnapshot {
            id: AuctionTournamentId::from_uuid(tournament.id),
            source_recruitment_id: AuctionRecruitmentId::from_uuid(
                tournament.source_recruitment_id,
            ),
            display_number: tournament.display_number,
            title: tournament.title,
            format,
            rules,
            status: tournament_status,
            revision: tournament.revision,
            href: format!(
                "/g/{}/auction/tournaments/{}/draft",
                guild_id.as_str(),
                tournament_id
            ),
        },
        teams: teams
            .into_iter()
            .map(|team| AuctionTournamentTeamSnapshot {
                id: team.id,
                team_index: team.team_index,
                team_name: team.team_name,
                initial_points: team.initial_points,
                current_points: team.current_points,
                point_revision: team.point_revision,
                members: members_by_team.remove(&team.id).unwrap_or_default(),
            })
            .collect(),
        candidates,
        draft: AuctionTournamentDraftStateSnapshot {
            current_target,
            current_target_profile,
            unsold,
            recent_action,
        },
        permissions: AuctionTournamentDraftPermissions {
            can_manage_draft: is_operator
                && tournament_status == AuctionTournamentStatus::Bidding
                && !has_bracket_artifacts,
            can_manage_bracket: false,
            can_revert_last,
            can_cancel: is_operator
                && matches!(
                    tournament_status,
                    AuctionTournamentStatus::Bidding
                        | AuctionTournamentStatus::BracketSetup
                        | AuctionTournamentStatus::InGame
                ),
        },
    })
}

async fn auction_tournament_current_target_profile(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    season_id: SeasonId,
    target: &AuctionTournamentCandidateSnapshot,
) -> Result<AuctionTournamentCurrentTargetProfileSnapshot, AttemptError> {
    let profile = sqlx::query_as::<_, AuctionTournamentCurrentTargetProfileRow>(
        "SELECT app_user.avatar_hash,
                member.main_lane,
                account.profile_icon_id,
                cached_riot.snapshot AS riot_snapshot,
                cached_riot.refreshed_at AS riot_refreshed_at,
                solo_rank.tier AS solo_rank_tier,
                solo_rank.division AS solo_rank_division
         FROM guild_members AS member
         JOIN users AS app_user
           ON app_user.user_id = member.user_id
          AND app_user.deleted_at IS NULL
         LEFT JOIN LATERAL (
             SELECT riot.platform, riot.puuid, riot.profile_icon_id
             FROM riot_accounts AS riot
             WHERE riot.user_id = member.user_id
               AND riot.is_main = true
               AND riot.origin = 'RIOT'
             ORDER BY riot.updated_at DESC
             LIMIT 1
         ) AS account ON TRUE
         LEFT JOIN LATERAL (
             SELECT records.snapshot, records.refreshed_at
             FROM riot_records_snapshots AS records
             WHERE records.platform = account.platform
               AND records.puuid = account.puuid
             LIMIT 1
         ) AS cached_riot ON TRUE
         LEFT JOIN LATERAL (
             SELECT rank_entry.value ->> 'tier' AS tier,
                    rank_entry.value ->> 'rank' AS division
             FROM riot_records_snapshots AS records
             CROSS JOIN LATERAL jsonb_array_elements(
                 CASE
                     WHEN jsonb_typeof(records.snapshot -> 'ranks') = 'array'
                         THEN records.snapshot -> 'ranks'
                     ELSE '[]'::jsonb
                 END
             ) AS rank_entry(value)
             WHERE records.platform = account.platform
               AND records.puuid = account.puuid
               AND rank_entry.value ->> 'queueType' = 'RANKED_SOLO_5x5'
             LIMIT 1
         ) AS solo_rank ON TRUE
         WHERE member.guild_id = $1 AND member.user_id = $2",
    )
    .bind(guild_id)
    .bind(&target.user_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)?;

    let lane_rows = sqlx::query_as::<_, AuctionTournamentCurrentTargetLaneMmrRow>(
        "SELECT lane, mmr, games_played, wins
         FROM user_lane_mmr
         WHERE guild_id = $1 AND season_id = $2 AND user_id = $3",
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(&target.user_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut lane_mmr_by_lane = HashMap::with_capacity(lane_rows.len());
    for row in lane_rows {
        if !row.mmr.is_finite()
            || row.games_played < 0
            || row.wins < 0
            || row.wins > row.games_played
            || lane_mmr_by_lane.insert(row.lane, row).is_some()
        {
            return Err(AttemptError::InvariantViolation);
        }
    }
    let lane_mmr = [
        Lane::Top,
        Lane::Jungle,
        Lane::Mid,
        Lane::Bottom,
        Lane::Support,
    ]
    .into_iter()
    .map(|lane| {
        let row = lane_mmr_by_lane.remove(&lane);
        AuctionTournamentCandidateLaneMmrSnapshot {
            lane,
            mmr: row.as_ref().map_or(1500.0, |value| value.mmr),
            games: row.as_ref().map_or(0, |value| value.games_played),
            wins: row.as_ref().map_or(0, |value| value.wins),
        }
    })
    .collect::<Vec<_>>();
    let games = lane_mmr
        .iter()
        .try_fold(0_i32, |total, entry| total.checked_add(entry.games))
        .ok_or(AttemptError::InvariantViolation)?;
    let wins = lane_mmr
        .iter()
        .try_fold(0_i32, |total, entry| total.checked_add(entry.wins))
        .ok_or(AttemptError::InvariantViolation)?;
    let losses = games
        .checked_sub(wins)
        .ok_or(AttemptError::InvariantViolation)?;

    let preference_lane = target.declared_lane.or(profile.main_lane);
    let champion_ids = match preference_lane {
        Some(lane) => sqlx::query_as::<_, AuctionTournamentCurrentTargetChampionRow>(
            "SELECT champion_id
             FROM user_champion_preferences
             WHERE user_id = $1 AND lane = $2
             ORDER BY position
             LIMIT 10",
        )
        .bind(&target.user_id)
        .bind(lane)
        .fetch_all(&mut **transaction)
        .await?
        .into_iter()
        .map(|row| row.champion_id)
        .collect(),
        None => Vec::new(),
    };
    let champion_records = auction_tournament_current_target_champion_records(
        transaction,
        guild_id,
        season_id,
        &target.user_id,
    )
    .await?;
    let recent_games = auction_tournament_current_target_recent_games(
        transaction,
        guild_id,
        season_id,
        &target.user_id,
    )
    .await?;

    Ok(AuctionTournamentCurrentTargetProfileSnapshot {
        avatar_url: auction_tournament_discord_avatar_url(
            &target.user_id,
            profile.avatar_hash.as_deref(),
        ),
        profile_icon_id: profile.profile_icon_id,
        riot_tier: compact_riot_tier(
            profile.solo_rank_tier.as_deref(),
            profile.solo_rank_division.as_deref(),
        ),
        main_lane: profile.main_lane,
        summary: AuctionTournamentCandidateRecordSummarySnapshot {
            games,
            wins,
            losses,
        },
        lane_mmr,
        champion_ids,
        champion_records,
        recent_games,
        riot: auction_tournament_current_target_riot(
            profile.riot_snapshot,
            profile.riot_refreshed_at,
        ),
    })
}

async fn auction_tournament_current_target_champion_records(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    season_id: SeasonId,
    user_id: &UserId,
) -> Result<Vec<AuctionTournamentCandidateChampionRecordSnapshot>, AttemptError> {
    let query = format!(
        "{AUCTION_TOURNAMENT_ELIGIBLE_RECORDS_CTE}
         SELECT champion_id,
                count(*) AS games,
                count(*) FILTER (WHERE won) AS wins
         FROM eligible
         WHERE champion_id IS NOT NULL
         GROUP BY champion_id
         ORDER BY count(*) DESC, count(*) FILTER (WHERE won) DESC, champion_id
         LIMIT 3"
    );
    // Both SQL fragments are static; guild, season, and user values stay bound below.
    let rows = sqlx::query_as::<_, AuctionTournamentCurrentTargetChampionRecordRow>(
        sqlx::AssertSqlSafe(query),
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await?;

    rows.into_iter()
        .map(|row| {
            if row.champion_id.is_empty() || row.champion_id.len() > MAX_CHAMPION_ID_BYTES {
                return Err(AttemptError::InvariantViolation);
            }
            let games = i32::try_from(row.games).map_err(|_| AttemptError::InvariantViolation)?;
            let wins = i32::try_from(row.wins).map_err(|_| AttemptError::InvariantViolation)?;
            if games < 0 || wins < 0 {
                return Err(AttemptError::InvariantViolation);
            }
            let losses = games
                .checked_sub(wins)
                .ok_or(AttemptError::InvariantViolation)?;
            Ok(AuctionTournamentCandidateChampionRecordSnapshot {
                champion_id: row.champion_id,
                games,
                wins,
                losses,
            })
        })
        .collect()
}

async fn auction_tournament_current_target_recent_games(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    season_id: SeasonId,
    user_id: &UserId,
) -> Result<Vec<AuctionTournamentCandidateRecentGameSnapshot>, AttemptError> {
    let query = format!(
        "{AUCTION_TOURNAMENT_ELIGIBLE_RECORDS_CTE}
         SELECT id, played_at, lane, champion_id, won
         FROM eligible
         ORDER BY played_at DESC, id DESC
         LIMIT 8"
    );
    // Both SQL fragments are static; guild, season, and user values stay bound below.
    let rows = sqlx::query_as::<_, AuctionTournamentCurrentTargetRecentGameRow>(
        sqlx::AssertSqlSafe(query),
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await?;

    rows.into_iter()
        .map(|row| {
            if row.champion_id.as_deref().is_some_and(|champion_id| {
                champion_id.is_empty() || champion_id.len() > MAX_CHAMPION_ID_BYTES
            }) {
                return Err(AttemptError::InvariantViolation);
            }
            Ok(AuctionTournamentCandidateRecentGameSnapshot {
                id: row.id,
                played_at: row.played_at,
                result: if row.won { "WIN" } else { "LOSS" }.to_owned(),
                lane: row.lane,
                champion_id: row.champion_id,
            })
        })
        .collect()
}

fn auction_tournament_current_target_riot(
    riot_snapshot: Option<Json<Value>>,
    riot_refreshed_at: Option<OffsetDateTime>,
) -> Option<AuctionTournamentCurrentTargetRiotSnapshot> {
    let records = serde_json::from_value::<RiotRecords>(riot_snapshot?.0).ok()?;
    let refreshed_at = riot_refreshed_at?;
    if records.validate(records.platform).is_err() {
        return None;
    }

    let (games, wins) = records
        .recent_matches
        .iter()
        .filter_map(|game| {
            game.participants
                .iter()
                .find(|participant| participant.puuid == game.target_puuid)
                .map(|participant| participant.win)
        })
        .fold((0_i32, 0_i32), |(games, wins), won| {
            (games + 1, wins + i32::from(won))
        });

    Some(AuctionTournamentCurrentTargetRiotSnapshot {
        games,
        wins,
        champion_ids: records
            .champion_aggregates
            .into_iter()
            .take(3)
            .map(|champion| champion.champion_name)
            .collect(),
        refreshed_at,
    })
}

fn auction_tournament_discord_avatar_url(
    user_id: &UserId,
    avatar_hash: Option<&str>,
) -> Option<String> {
    let hash = avatar_hash?;
    let valid_hash = !hash.is_empty()
        && hash.len() <= 128
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'_');
    valid_hash.then(|| {
        format!(
            "https://cdn.discordapp.com/avatars/{}/{hash}.webp?size=128",
            user_id.as_str()
        )
    })
}

async fn auction_tournament_bracket_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    viewer_id: &UserId,
) -> Result<AuctionTournamentBracketSnapshot, AttemptError> {
    require_active_session_member_read(transaction, guild_id, viewer_id).await?;
    let tournament = sqlx::query_as::<_, AuctionTournamentBracketRow>(
        "SELECT id, display_number, title, format, semifinal_best_of, final_best_of,
                hard_fearless, status, revision, bracket_locked_at
         FROM auction_tournaments
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)?;
    if !matches!(
        tournament.status,
        AuctionTournamentStatus::BracketSetup
            | AuctionTournamentStatus::InGame
            | AuctionTournamentStatus::Completed
            | AuctionTournamentStatus::Cancelled
    ) {
        return Err(AttemptError::AuctionTournamentBracketUnavailable(
            tournament.status,
        ));
    }
    let format = parse_auction_format(&tournament.format)?;
    let rules = AuctionRecruitmentSeriesRules {
        hard_fearless: tournament.hard_fearless,
        semifinal_best_of: tournament.semifinal_best_of,
        final_best_of: tournament.final_best_of,
    };
    validate_auction_recruitment_series_rules(format, &rules)
        .map_err(|_| AttemptError::InvariantViolation)?;
    let teams = sqlx::query_as::<_, AuctionTournamentTeamRow>(
        "SELECT id, team_index, team_name, initial_points, current_points, point_revision
         FROM auction_teams
         WHERE guild_id = $1 AND tournament_id = $2
         ORDER BY team_index",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let member_rows = sqlx::query_as::<_, AuctionTournamentTeamMemberRow>(
        "SELECT member.team_id, member.id, member.user_id, guild_member.display_name,
                member.declared_lane, member.mmr_at_acquisition, member.snapshot_origin,
                member.acquired_via, member.roster_slot
         FROM auction_team_members AS member
         JOIN guild_members AS guild_member
           ON guild_member.guild_id = member.guild_id
          AND guild_member.user_id = member.user_id
         WHERE member.guild_id = $1
           AND member.tournament_id = $2
           AND member.released_at IS NULL
         ORDER BY member.team_id, member.roster_slot",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let slot_rows = sqlx::query_as::<_, AuctionTournamentBracketSlotRow>(
        "SELECT slot_key, team_id
         FROM auction_bracket_slots
         WHERE guild_id = $1 AND tournament_id = $2
         ORDER BY CASE slot_key
                      WHEN 'A_0' THEN 0
                      WHEN 'A_1' THEN 1
                      WHEN 'B_0' THEN 2
                      WHEN 'B_1' THEN 3
                      ELSE 4
                  END",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let match_rows = sqlx::query_as::<_, AuctionTournamentBracketMatchRow>(
        "SELECT auction_match.id,
                auction_match.match_key,
                auction_match.best_of,
                auction_match.hard_fearless,
                auction_match.left_team_id,
                auction_match.right_team_id,
                auction_match.status,
                auction_match.revision,
                auction_match.winner_team_id,
                auction_match.lineups_locked_at,
                count(game.id) FILTER (
                    WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE')
                       OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                ) AS left_score,
                count(game.id) FILTER (
                    WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED')
                       OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                ) AS right_score
         FROM auction_matches AS auction_match
         LEFT JOIN games AS game
           ON game.guild_id = auction_match.guild_id
          AND game.auction_match_id = auction_match.id
          AND game.status = 'CONFIRMED'
         WHERE auction_match.guild_id = $1
           AND auction_match.tournament_id = $2
           AND auction_match.deleted_at IS NULL
           AND auction_match.status <> 'CANCELLED'
         GROUP BY auction_match.id,
                  auction_match.match_key,
                  auction_match.best_of,
                  auction_match.hard_fearless,
                  auction_match.left_team_id,
                  auction_match.right_team_id,
                  auction_match.status,
                  auction_match.revision,
                  auction_match.winner_team_id,
                  auction_match.lineups_locked_at
         ORDER BY CASE auction_match.match_key
                      WHEN 'SEMIFINAL_A' THEN 0
                      WHEN 'SEMIFINAL_B' THEN 1
                      WHEN 'FINAL' THEN 2
                      ELSE 3
                  END,
                  auction_match.id",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let mut members_by_team = HashMap::<Uuid, Vec<AuctionTournamentTeamMemberSnapshot>>::new();
    for member in member_rows {
        members_by_team.entry(member.team_id).or_default().push(
            AuctionTournamentTeamMemberSnapshot {
                team_member_id: member.id,
                user_id: member.user_id,
                display_name: member.display_name,
                declared_lane: member.declared_lane,
                mmr_at_acquisition: member.mmr_at_acquisition,
                snapshot_origin: member.snapshot_origin,
                acquired_via: member.acquired_via,
                roster_slot: member.roster_slot,
            },
        );
    }
    let mut slots_by_key = BTreeMap::new();
    for slot in slot_rows {
        if !is_auction_bracket_slot_key(&slot.slot_key)
            || slots_by_key.insert(slot.slot_key, slot.team_id).is_some()
        {
            return Err(AttemptError::InvariantViolation);
        }
    }
    let slots = AUCTION_BRACKET_SLOT_KEYS
        .iter()
        .map(|slot_key| AuctionTournamentBracketSlotSnapshot {
            slot_key: (*slot_key).to_owned(),
            team_id: slots_by_key.remove(*slot_key).flatten(),
        })
        .collect::<Vec<_>>();
    if !slots_by_key.is_empty() {
        return Err(AttemptError::InvariantViolation);
    }
    let matches = match_rows
        .into_iter()
        .map(|auction_match| {
            if !is_auction_match_key(&auction_match.match_key)
                || !is_auction_match_status(&auction_match.status)
                || auction_match.left_score < 0
                || auction_match.right_score < 0
            {
                return Err(AttemptError::InvariantViolation);
            }
            Ok(AuctionTournamentBracketMatchSnapshot {
                id: auction_match.id,
                match_key: auction_match.match_key,
                best_of: auction_match.best_of,
                hard_fearless: auction_match.hard_fearless,
                left_team_id: auction_match.left_team_id,
                right_team_id: auction_match.right_team_id,
                status: auction_match.status,
                revision: auction_match.revision,
                left_score: auction_match.left_score,
                right_score: auction_match.right_score,
                winner_team_id: auction_match.winner_team_id,
                lineups_locked: auction_match.lineups_locked_at.is_some(),
            })
        })
        .collect::<Result<Vec<_>, AttemptError>>()?;
    let is_operator = is_active_session_operator(transaction, guild_id, viewer_id).await?;
    let locked = tournament.bracket_locked_at.is_some();
    let tournament_status = tournament.status;
    let has_match_artifacts = !matches.is_empty();
    let has_result_artifacts =
        auction_tournament_matches_have_result_artifacts(transaction, guild_id, tournament_id)
            .await?;
    let has_latest_revertible_draft_action =
        auction_tournament_has_latest_revertible_draft_action(transaction, guild_id, tournament_id)
            .await?;

    Ok(AuctionTournamentBracketSnapshot {
        tournament: AuctionTournamentBracketSummarySnapshot {
            id: AuctionTournamentId::from_uuid(tournament.id),
            title: tournament.title,
            display_number: tournament.display_number,
            format,
            rules,
            status: tournament_status,
            revision: tournament.revision,
            href: format!(
                "/g/{}/auction/tournaments/{}/bracket",
                guild_id.as_str(),
                tournament_id
            ),
        },
        teams: teams
            .into_iter()
            .map(|team| AuctionTournamentBracketTeamSnapshot {
                id: team.id,
                team_index: team.team_index,
                team_name: team.team_name,
                initial_points: team.initial_points,
                current_points: team.current_points,
                members: members_by_team.remove(&team.id).unwrap_or_default(),
            })
            .collect(),
        bracket: AuctionTournamentBracketStateSnapshot {
            locked,
            slots,
            matches,
        },
        permissions: AuctionTournamentBracketPermissions {
            can_manage_bracket: is_operator
                && tournament_status == AuctionTournamentStatus::BracketSetup
                && !locked
                && !has_match_artifacts,
            can_manage_matches: false,
            can_recover_bracket: is_operator
                && locked
                && has_match_artifacts
                && matches!(
                    tournament_status,
                    AuctionTournamentStatus::BracketSetup | AuctionTournamentStatus::InGame
                )
                && !has_result_artifacts,
            can_revert_draft: is_operator
                && has_latest_revertible_draft_action
                && tournament_status == AuctionTournamentStatus::BracketSetup
                && !locked
                && !has_match_artifacts,
            can_cancel: is_operator
                && matches!(
                    tournament_status,
                    AuctionTournamentStatus::Bidding
                        | AuctionTournamentStatus::BracketSetup
                        | AuctionTournamentStatus::InGame
                ),
        },
    })
}

async fn auction_match_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    viewer_id: &UserId,
) -> Result<AuctionMatchSnapshot, AttemptError> {
    require_active_session_member_read(transaction, guild_id, viewer_id).await?;
    let auction_match = sqlx::query_as::<_, AuctionMatchSnapshotRow>(
        "SELECT auction_match.id,
                auction_match.tournament_id,
                auction_match.match_key,
                auction_match.best_of,
                auction_match.hard_fearless,
                auction_match.status,
                auction_match.revision,
                auction_match.left_team_id,
                auction_match.right_team_id,
                auction_match.winner_team_id,
                auction_match.lineups_locked_at,
                auction_match.lineups_locked_by,
                count(game.id) FILTER (
                    WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE')
                       OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                ) AS left_score,
                count(game.id) FILTER (
                    WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED')
                       OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                ) AS right_score
         FROM auction_matches AS auction_match
         LEFT JOIN games AS game
           ON game.guild_id = auction_match.guild_id
          AND game.auction_match_id = auction_match.id
          AND game.status = 'CONFIRMED'
         WHERE auction_match.guild_id = $1
           AND auction_match.id = $2
           AND auction_match.deleted_at IS NULL
         GROUP BY auction_match.id,
                  auction_match.tournament_id,
                  auction_match.match_key,
                  auction_match.best_of,
                  auction_match.hard_fearless,
                  auction_match.status,
                  auction_match.revision,
                  auction_match.left_team_id,
                  auction_match.right_team_id,
                  auction_match.winner_team_id,
                  auction_match.lineups_locked_at,
                  auction_match.lineups_locked_by",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionMatchNotFound)?;
    let left_team_id = auction_match
        .left_team_id
        .ok_or(AttemptError::AuctionMatchTeamsPending)?;
    let right_team_id = auction_match
        .right_team_id
        .ok_or(AttemptError::AuctionMatchTeamsPending)?;
    if left_team_id == right_team_id
        || !is_auction_match_key(&auction_match.match_key)
        || !is_auction_match_status(&auction_match.status)
        || !matches!(auction_match.best_of, 1 | 3 | 5)
        || auction_match.revision < 0
        || auction_match.left_score < 0
        || auction_match.right_score < 0
    {
        return Err(AttemptError::InvariantViolation);
    }
    if auction_match
        .winner_team_id
        .is_some_and(|winner| winner != left_team_id && winner != right_team_id)
    {
        return Err(AttemptError::InvariantViolation);
    }

    let team_rows = sqlx::query_as::<_, AuctionTournamentTeamRow>(
        "SELECT id, team_index, team_name, initial_points, current_points, point_revision
         FROM auction_teams
         WHERE guild_id = $1
           AND tournament_id = $2
           AND (id = $3 OR id = $4)
         ORDER BY CASE id WHEN $3 THEN 0 WHEN $4 THEN 1 ELSE 2 END",
    )
    .bind(guild_id)
    .bind(auction_match.tournament_id)
    .bind(left_team_id)
    .bind(right_team_id)
    .fetch_all(&mut **transaction)
    .await?;
    if team_rows.len() != 2 || team_rows[0].id != left_team_id || team_rows[1].id != right_team_id {
        return Err(AttemptError::InvariantViolation);
    }
    let member_rows = sqlx::query_as::<_, AuctionTournamentTeamMemberRow>(
        "SELECT member.team_id, member.id, member.user_id, guild_member.display_name,
                member.declared_lane, member.mmr_at_acquisition, member.snapshot_origin,
                member.acquired_via, member.roster_slot
         FROM auction_team_members AS member
         JOIN guild_members AS guild_member
           ON guild_member.guild_id = member.guild_id
          AND guild_member.user_id = member.user_id
         WHERE member.guild_id = $1
           AND member.tournament_id = $2
           AND member.released_at IS NULL
           AND (member.team_id = $3 OR member.team_id = $4)
         ORDER BY CASE member.team_id WHEN $3 THEN 0 WHEN $4 THEN 1 ELSE 2 END,
                  member.roster_slot",
    )
    .bind(guild_id)
    .bind(auction_match.tournament_id)
    .bind(left_team_id)
    .bind(right_team_id)
    .fetch_all(&mut **transaction)
    .await?;
    let lineup_rows = sqlx::query_as::<_, AuctionMatchLineupRow>(
        "SELECT team_id, lane, team_member_id, user_id
         FROM auction_match_lineups
         WHERE guild_id = $1 AND tournament_id = $2 AND match_id = $3
         ORDER BY CASE team_id WHEN $4 THEN 0 WHEN $5 THEN 1 ELSE 2 END,
                  CASE lane
                      WHEN 'TOP' THEN 0
                      WHEN 'JUNGLE' THEN 1
                      WHEN 'MID' THEN 2
                      WHEN 'BOTTOM' THEN 3
                      WHEN 'SUPPORT' THEN 4
                      ELSE 5
                  END",
    )
    .bind(guild_id)
    .bind(auction_match.tournament_id)
    .bind(match_id.as_uuid())
    .bind(left_team_id)
    .bind(right_team_id)
    .fetch_all(&mut **transaction)
    .await?;

    let team_ids = HashSet::from([left_team_id, right_team_id]);
    let mut members_by_team = HashMap::<Uuid, Vec<AuctionTournamentTeamMemberSnapshot>>::new();
    let mut member_users = HashMap::<(Uuid, Uuid), UserId>::new();
    let mut roster_users = HashSet::new();
    for member in member_rows {
        if !team_ids.contains(&member.team_id)
            || !(1..=5).contains(&member.roster_slot)
            || !roster_users.insert(member.user_id.clone())
            || member_users
                .insert((member.team_id, member.id), member.user_id.clone())
                .is_some()
        {
            return Err(AttemptError::InvariantViolation);
        }
        members_by_team.entry(member.team_id).or_default().push(
            AuctionTournamentTeamMemberSnapshot {
                team_member_id: member.id,
                user_id: member.user_id,
                display_name: member.display_name,
                declared_lane: member.declared_lane,
                mmr_at_acquisition: member.mmr_at_acquisition,
                snapshot_origin: member.snapshot_origin,
                acquired_via: member.acquired_via,
                roster_slot: member.roster_slot,
            },
        );
    }
    let mut lineup_slots = HashSet::new();
    let mut lineup_members = HashSet::new();
    let mut lineup_users = HashSet::new();
    let lineups = lineup_rows
        .into_iter()
        .map(|lineup| {
            if !team_ids.contains(&lineup.team_id)
                || !lineup_slots.insert((lineup.team_id, lineup.lane))
                || !lineup_members.insert(lineup.team_member_id)
                || !lineup_users.insert(lineup.user_id.clone())
                || member_users.get(&(lineup.team_id, lineup.team_member_id))
                    != Some(&lineup.user_id)
            {
                return Err(AttemptError::InvariantViolation);
            }
            Ok(AuctionMatchLineupAssignmentSnapshot {
                team_id: lineup.team_id,
                lane: lineup.lane,
                team_member_id: lineup.team_member_id,
                user_id: lineup.user_id,
            })
        })
        .collect::<Result<Vec<_>, AttemptError>>()?;
    if auction_match.lineups_locked_at.is_some()
        && !has_complete_auction_match_lineup(&lineups, left_team_id, right_team_id)
    {
        return Err(AttemptError::InvariantViolation);
    }
    let lineup_player_ids = lineups
        .iter()
        .map(|lineup| lineup.user_id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let games =
        auction_match_confirmed_game_snapshots(transaction, guild_id, match_id, &lineup_player_ids)
            .await?;
    if !games.is_empty()
        && (!auction_match.lineups_locked_at.is_some()
            || !has_complete_auction_match_lineup(&lineups, left_team_id, right_team_id))
    {
        return Err(AttemptError::InvariantViolation);
    }
    let semifinal_lineups_ready = if is_auction_semifinal_match_key(&auction_match.match_key) {
        twenty_person_semifinal_lineups_are_locked(
            transaction,
            guild_id,
            AuctionTournamentId::from_uuid(auction_match.tournament_id),
        )
        .await?
    } else {
        true
    };
    let has_result_artifacts =
        auction_match_has_result_artifacts(transaction, guild_id, match_id).await?;
    let is_operator = is_active_session_operator(transaction, guild_id, viewer_id).await?;
    let downstream_final_blocks_revert = if is_operator
        && auction_match.status == "COMPLETED"
        && is_auction_semifinal_match_key(&auction_match.match_key)
    {
        auction_completed_twenty_person_semifinal_revert_is_blocked(
            transaction,
            guild_id,
            AuctionTournamentId::from_uuid(auction_match.tournament_id),
            &auction_match.match_key,
            auction_match.winner_team_id,
        )
        .await?
    } else {
        false
    };
    let can_manage_lineup = is_operator
        && auction_match.status == "LINEUP_PENDING"
        && auction_match.lineups_locked_at.is_none();
    let can_enter_results = can_enter_auction_match_results(
        is_operator,
        auction_match.lineups_locked_at.is_some(),
        semifinal_lineups_ready,
        &auction_match.status,
    );
    let can_revert_last_game = can_revert_auction_match_last_game(
        is_operator,
        auction_match.lineups_locked_at.is_some(),
        !games.is_empty(),
        &auction_match.status,
        downstream_final_blocks_revert,
    );
    let can_unlock_lineup = is_operator
        && auction_match.status == "READY"
        && auction_match.lineups_locked_at.is_some()
        && auction_match.lineups_locked_by.is_some()
        && !has_result_artifacts;

    Ok(AuctionMatchSnapshot {
        r#match: AuctionMatchSnapshotMatch {
            id: AuctionMatchId::from_uuid(auction_match.id),
            tournament_id: AuctionTournamentId::from_uuid(auction_match.tournament_id),
            label: auction_match_label(&auction_match.match_key)?.to_owned(),
            match_key: auction_match.match_key,
            best_of: auction_match.best_of,
            hard_fearless: auction_match.hard_fearless,
            status: auction_match.status,
            revision: auction_match.revision,
            left_team_id,
            right_team_id,
            left_score: auction_match.left_score,
            right_score: auction_match.right_score,
            winner_team_id: auction_match.winner_team_id,
            lineups_locked: auction_match.lineups_locked_at.is_some(),
        },
        teams: team_rows
            .into_iter()
            .map(|team| AuctionMatchSnapshotTeam {
                id: team.id,
                team_name: team.team_name,
                members: members_by_team.remove(&team.id).unwrap_or_default(),
            })
            .collect(),
        lineups,
        games,
        permissions: AuctionMatchSnapshotPermissions {
            can_manage_lineup,
            can_enter_results,
            can_unlock_lineup,
            can_revert_last_game,
        },
    })
}

async fn auction_match_confirmed_game_snapshots(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    player_ids: &BTreeSet<String>,
) -> Result<Vec<AuctionMatchConfirmedGameSnapshot>, AttemptError> {
    let rows = sqlx::query_as::<_, AuctionConfirmedGameRow>(
        "SELECT game.id, game.source_input_id, game.game_number AS set_number,
                game.side_preset, game.winning_side, game.patch_version
         FROM games AS game
         WHERE game.guild_id = $1
           AND game.auction_match_id = $2
           AND game.status = 'CONFIRMED'
         ORDER BY game.game_number",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let mut games = Vec::with_capacity(rows.len());
    for row in rows {
        let input = SeriesPickBanGameInputRow {
            id: row.source_input_id,
            set_number: row.set_number,
            side_preset: row.side_preset,
            side_selected: true,
            winning_side: Some(row.winning_side),
            patch_version: row.patch_version,
        };
        let snapshot =
            series_pick_ban_game_snapshot(transaction, guild_id, &input, player_ids).await?;
        if snapshot.picks.keys().cloned().collect::<BTreeSet<_>>() != *player_ids {
            return Err(AttemptError::InvariantViolation);
        }
        games.push(AuctionMatchConfirmedGameSnapshot {
            id: GameId::from_uuid(row.id),
            set_number: snapshot.set_number,
            side_preset: snapshot.side_preset,
            winner: snapshot.winner.ok_or(AttemptError::InvariantViolation)?,
            picks: snapshot.picks,
            bans: snapshot.bans,
            patch_version: snapshot.patch_version,
        });
    }
    Ok(games)
}

async fn auction_tournament_result_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    viewer_id: &UserId,
) -> Result<AuctionTournamentResultSnapshot, AttemptError> {
    require_active_session_member_read(transaction, guild_id, viewer_id).await?;
    let tournament = sqlx::query_as::<_, AuctionTournamentResultRow>(
        "SELECT id, display_number, title, format, semifinal_best_of, final_best_of,
                hard_fearless, status, revision, champion_team_id, bracket_locked_at
         FROM auction_tournaments
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)?;
    if !matches!(
        tournament.status,
        AuctionTournamentStatus::InGame
            | AuctionTournamentStatus::Completed
            | AuctionTournamentStatus::Cancelled
    ) {
        return Err(AttemptError::AuctionTournamentResultUnavailable(
            tournament.status,
        ));
    }
    let format = parse_auction_format(&tournament.format)?;
    let rules = AuctionRecruitmentSeriesRules {
        hard_fearless: tournament.hard_fearless,
        semifinal_best_of: tournament.semifinal_best_of,
        final_best_of: tournament.final_best_of,
    };
    validate_auction_recruitment_series_rules(format, &rules)
        .map_err(|_| AttemptError::InvariantViolation)?;

    let team_rows = sqlx::query_as::<_, AuctionTournamentTeamRow>(
        "SELECT id, team_index, team_name, initial_points, current_points, point_revision
         FROM auction_teams
         WHERE guild_id = $1 AND tournament_id = $2
         ORDER BY team_index",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    if team_rows.len() != usize::try_from(format.team_count()).unwrap_or_default()
        || team_rows.iter().enumerate().any(|(index, row)| {
            row.team_index != i16::try_from(index + 1).unwrap_or_default()
                || row.current_points < 0
                || row.initial_points < row.current_points
        })
    {
        return Err(AttemptError::InvariantViolation);
    }
    let member_rows = sqlx::query_as::<_, AuctionTournamentResultMemberRow>(
        "SELECT member.team_id,
                member.id AS team_member_id,
                member.user_id,
                guild_member.display_name,
                member.declared_lane,
                member.acquired_via,
                action.points AS acquisition_points,
                member.roster_slot
         FROM auction_team_members AS member
         JOIN guild_members AS guild_member
           ON guild_member.guild_id = member.guild_id
          AND guild_member.user_id = member.user_id
         LEFT JOIN auction_actions AS action
           ON action.guild_id = member.guild_id
          AND action.tournament_id = member.tournament_id
          AND action.id = member.acquisition_action_id
         WHERE member.guild_id = $1
           AND member.tournament_id = $2
           AND member.released_at IS NULL
         ORDER BY member.team_id, member.roster_slot",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let team_ids = team_rows.iter().map(|row| row.id).collect::<HashSet<_>>();
    let mut members_by_team =
        HashMap::<Uuid, Vec<AuctionTournamentResultTeamMemberSnapshot>>::new();
    let mut member_by_team_id = HashMap::<(Uuid, Uuid), UserId>::new();
    let mut all_users = HashSet::new();
    for row in member_rows {
        if !team_ids.contains(&row.team_id)
            || !(1..=5).contains(&row.roster_slot)
            || row.acquisition_points.is_some_and(|points| points < 0)
            || member_by_team_id
                .insert((row.team_id, row.team_member_id), row.user_id.clone())
                .is_some()
            || !all_users.insert(row.user_id.clone())
        {
            return Err(AttemptError::InvariantViolation);
        }
        members_by_team.entry(row.team_id).or_default().push(
            AuctionTournamentResultTeamMemberSnapshot {
                team_member_id: row.team_member_id,
                user_id: row.user_id,
                display_name: row.display_name,
                declared_lane: row.declared_lane,
                acquired_via: row.acquired_via,
                acquisition_points: row.acquisition_points,
            },
        );
    }
    let teams = team_rows
        .into_iter()
        .map(|row| {
            let members = members_by_team.remove(&row.id).unwrap_or_default();
            if members.len() > 5 {
                return Err(AttemptError::InvariantViolation);
            }
            Ok(AuctionTournamentResultTeamSnapshot {
                id: row.id,
                team_index: row.team_index,
                team_name: row.team_name,
                initial_points: row.initial_points,
                current_points: row.current_points,
                spent_points: row
                    .initial_points
                    .checked_sub(row.current_points)
                    .ok_or(AttemptError::InvariantViolation)?,
                members,
            })
        })
        .collect::<Result<Vec<_>, AttemptError>>()?;

    // Bracket recovery keeps replaced match rows as CANCELLED history. They are not part of an
    // active or completed tournament's current bracket, but remain visible in a cancelled
    // tournament archive.
    let include_cancelled_match_history = tournament.status == AuctionTournamentStatus::Cancelled;
    let match_rows = sqlx::query_as::<_, AuctionTournamentResultMatchRow>(
        "SELECT auction_match.id,
                auction_match.match_key,
                auction_match.best_of,
                auction_match.hard_fearless,
                auction_match.status,
                auction_match.left_team_id,
                auction_match.right_team_id,
                auction_match.winner_team_id,
                auction_match.lineups_locked_at,
                count(game.id) FILTER (
                    WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE')
                       OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                ) AS left_score,
                count(game.id) FILTER (
                    WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED')
                       OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                ) AS right_score
         FROM auction_matches AS auction_match
         LEFT JOIN games AS game
           ON game.guild_id = auction_match.guild_id
          AND game.auction_match_id = auction_match.id
          AND game.status = 'CONFIRMED'
         WHERE auction_match.guild_id = $1
           AND auction_match.tournament_id = $2
           AND auction_match.deleted_at IS NULL
           AND ($3::boolean OR auction_match.status <> 'CANCELLED')
         GROUP BY auction_match.id, auction_match.match_key, auction_match.best_of,
                  auction_match.hard_fearless, auction_match.status,
                  auction_match.left_team_id, auction_match.right_team_id,
                  auction_match.winner_team_id, auction_match.lineups_locked_at
         ORDER BY CASE auction_match.match_key
                      WHEN 'SEMIFINAL_A' THEN 1
                      WHEN 'SEMIFINAL_B' THEN 2
                      WHEN 'FINAL' THEN 3
                      ELSE 4
                  END,
                  auction_match.id",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(include_cancelled_match_history)
    .fetch_all(&mut **transaction)
    .await?;
    let expected_match_keys: &[&str] = match format {
        AuctionFormat::TenPerson => &["FINAL"],
        AuctionFormat::TwentyPerson => &["SEMIFINAL_A", "SEMIFINAL_B", "FINAL"],
    };
    // A cancelled tournament can be stopped while bidding (no matches), or after a
    // bracket recovery left prior CANCELLED match history. Keep the archive readable
    // without weakening the active/completed bracket shape invariant.
    if tournament.status != AuctionTournamentStatus::Cancelled
        && (match_rows.len() != expected_match_keys.len()
            || match_rows
                .iter()
                .map(|row| row.match_key.as_str())
                .ne(expected_match_keys.iter().copied()))
    {
        return Err(AttemptError::InvariantViolation);
    }
    let match_ids = match_rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let lineup_rows = sqlx::query_as::<_, AuctionTournamentResultLineupRow>(
        "SELECT match_id, team_id, lane, team_member_id, user_id
         FROM auction_match_lineups
         WHERE guild_id = $1 AND tournament_id = $2 AND match_id = ANY($3)
         ORDER BY match_id, team_id,
                  CASE lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                      ELSE 6
                  END",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(&match_ids)
    .fetch_all(&mut **transaction)
    .await?;
    let mut lineups_by_match = HashMap::<Uuid, Vec<AuctionMatchLineupAssignmentSnapshot>>::new();
    for row in lineup_rows {
        if member_by_team_id.get(&(row.team_id, row.team_member_id)) != Some(&row.user_id) {
            return Err(AttemptError::InvariantViolation);
        }
        lineups_by_match.entry(row.match_id).or_default().push(
            AuctionMatchLineupAssignmentSnapshot {
                team_id: row.team_id,
                lane: row.lane,
                team_member_id: row.team_member_id,
                user_id: row.user_id,
            },
        );
    }
    let mut matches = Vec::with_capacity(match_rows.len());
    for row in match_rows {
        if !is_auction_match_status(&row.status)
            || !matches!(row.best_of, 1 | 3 | 5)
            || row.left_score < 0
            || row.right_score < 0
            || row
                .left_team_id
                .is_some_and(|team_id| !team_ids.contains(&team_id))
            || row
                .right_team_id
                .is_some_and(|team_id| !team_ids.contains(&team_id))
            || row
                .winner_team_id
                .is_some_and(|team_id| !team_ids.contains(&team_id))
        {
            return Err(AttemptError::InvariantViolation);
        }
        let lineups = lineups_by_match.remove(&row.id).unwrap_or_default();
        let lineups_locked = row.lineups_locked_at.is_some();
        if lineups_locked {
            let (left_team_id, right_team_id) = (row.left_team_id, row.right_team_id);
            let (Some(left_team_id), Some(right_team_id)) = (left_team_id, right_team_id) else {
                return Err(AttemptError::InvariantViolation);
            };
            if !has_complete_auction_match_lineup(&lineups, left_team_id, right_team_id) {
                return Err(AttemptError::InvariantViolation);
            }
        }
        let player_ids = lineups
            .iter()
            .map(|lineup| lineup.user_id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let match_id = AuctionMatchId::from_uuid(row.id);
        let games =
            auction_match_confirmed_game_snapshots(transaction, guild_id, match_id, &player_ids)
                .await?;
        if !games.is_empty() && !lineups_locked {
            return Err(AttemptError::InvariantViolation);
        }
        matches.push(AuctionTournamentResultMatchSnapshot {
            id: match_id,
            match_key: row.match_key,
            best_of: row.best_of,
            hard_fearless: row.hard_fearless,
            status: row.status,
            left_team_id: row.left_team_id,
            right_team_id: row.right_team_id,
            left_score: row.left_score,
            right_score: row.right_score,
            winner_team_id: row.winner_team_id,
            lineups_locked,
            lineups,
            games,
        });
    }
    if !lineups_by_match.is_empty() {
        return Err(AttemptError::InvariantViolation);
    }
    let player_rows = sqlx::query_as::<_, AuctionTournamentResultPlayerRow>(
        "SELECT member.user_id,
                guild_member.display_name,
                member.team_id,
                count(participant.game_id) FILTER (WHERE game.status = 'CONFIRMED') AS games,
                count(participant.game_id) FILTER (
                    WHERE game.status = 'CONFIRMED'
                      AND ((game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE' AND participant.team = 'TEAM_1')
                        OR (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED' AND participant.team = 'TEAM_2')
                        OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED' AND participant.team = 'TEAM_1')
                        OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE' AND participant.team = 'TEAM_2'))
                ) AS wins,
                sum(ledger.delta) FILTER (
                    WHERE game.status = 'CONFIRMED' AND ledger.source_type = 'GAME_RESULT'
                ) AS mmr_delta
         FROM auction_team_members AS member
         JOIN guild_members AS guild_member
           ON guild_member.guild_id = member.guild_id
          AND guild_member.user_id = member.user_id
         LEFT JOIN game_participants AS participant
           ON participant.guild_id = member.guild_id
          AND participant.user_id = member.user_id
         LEFT JOIN games AS game
           ON game.guild_id = participant.guild_id
          AND game.id = participant.game_id
          AND game.auction_match_id = ANY($3)
         LEFT JOIN mmr_ledger_entries AS ledger
           ON ledger.guild_id = game.guild_id
          AND ledger.game_id = game.id
          AND ledger.user_id = member.user_id
          AND ledger.lane = participant.lane
         WHERE member.guild_id = $1
           AND member.tournament_id = $2
           AND member.released_at IS NULL
         GROUP BY member.user_id, guild_member.display_name, member.team_id
         ORDER BY member.team_id, member.user_id",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(&match_ids)
    .fetch_all(&mut **transaction)
    .await?;
    let players = player_rows
        .into_iter()
        .map(|row| {
            if !team_ids.contains(&row.team_id)
                || row.games < 0
                || row.wins < 0
                || row.wins > row.games
            {
                return Err(AttemptError::InvariantViolation);
            }
            Ok(AuctionTournamentResultPlayerSnapshot {
                user_id: row.user_id,
                display_name: row.display_name,
                team_id: row.team_id,
                games: row.games,
                wins: row.wins,
                losses: row.games - row.wins,
                mmr_delta: row.mmr_delta,
            })
        })
        .collect::<Result<Vec<_>, AttemptError>>()?;
    let is_operator = is_active_session_operator(transaction, guild_id, viewer_id).await?;
    let has_match_artifacts = !matches.is_empty();
    let has_result_artifacts =
        auction_tournament_matches_have_result_artifacts(transaction, guild_id, tournament_id)
            .await?;
    Ok(AuctionTournamentResultSnapshot {
        tournament: AuctionTournamentResultTournamentSnapshot {
            id: AuctionTournamentId::from_uuid(tournament.id),
            title: tournament.title,
            display_number: tournament.display_number,
            format,
            rules,
            status: tournament.status,
            revision: tournament.revision,
            champion_team_id: tournament.champion_team_id,
            href: format!("/g/{guild_id}/auction/tournaments/{tournament_id}/result"),
        },
        teams,
        matches,
        players,
        permissions: AuctionTournamentResultPermissions {
            can_revert_results: is_operator
                && matches!(
                    tournament.status,
                    AuctionTournamentStatus::InGame | AuctionTournamentStatus::Completed
                ),
            can_recover_bracket: is_operator
                && tournament.bracket_locked_at.is_some()
                && has_match_artifacts
                && matches!(
                    tournament.status,
                    AuctionTournamentStatus::BracketSetup | AuctionTournamentStatus::InGame
                )
                && !has_result_artifacts,
            can_cancel: is_operator
                && matches!(
                    tournament.status,
                    AuctionTournamentStatus::Bidding
                        | AuctionTournamentStatus::BracketSetup
                        | AuctionTournamentStatus::InGame
                ),
        },
    })
}

fn normalize_auction_setup_team_inputs(
    inputs: &[AuctionSetupDraftTeamInput],
    team_count: i16,
    joined: &[AuctionRecruitmentParticipantLockRow],
) -> Result<Vec<ConfiguredAuctionSetupTeam>, AttemptError> {
    if inputs.len() != usize::try_from(team_count).unwrap_or_default() {
        return Err(AttemptError::AuctionSetupTeamCountInvalid {
            expected: team_count,
            actual: inputs.len(),
        });
    }
    let joined_ids = joined
        .iter()
        .map(|participant| participant.user_id.clone())
        .collect::<HashSet<_>>();
    let mut captains = HashSet::new();
    let mut normalized = Vec::with_capacity(inputs.len());
    for (position, team) in inputs.iter().enumerate() {
        let expected_index =
            i16::try_from(position + 1).map_err(|_| AttemptError::InvariantViolation)?;
        if team.team_index != expected_index {
            return Err(AttemptError::AuctionSetupTeamIndexInvalid {
                expected: expected_index,
                actual: team.team_index,
            });
        }
        let team_name = team.team_name.trim();
        if team_name.is_empty() || team_name.len() > MAX_AUCTION_TEAM_NAME_BYTES {
            return Err(AttemptError::AuctionSetupTeamNameInvalid);
        }
        if team.initial_points <= 0 {
            return Err(AttemptError::AuctionSetupInitialPointsInvalid);
        }
        if !captains.insert(team.captain_user_id.clone()) {
            return Err(AttemptError::AuctionSetupDuplicateCaptain);
        }
        if !joined_ids.contains(&team.captain_user_id) {
            return Err(AttemptError::AuctionSetupCaptainNotJoined);
        }
        normalized.push(ConfiguredAuctionSetupTeam {
            team_index: team.team_index,
            team_name: team_name.to_owned(),
            captain_user_id: team.captain_user_id.clone(),
            initial_points: team.initial_points,
        });
    }
    Ok(normalized)
}

fn validate_stored_auction_setup_teams(
    rows: &[AuctionSetupTeamRow],
    team_count: i16,
    joined: &[AuctionRecruitmentParticipantLockRow],
) -> Result<Vec<ConfiguredAuctionSetupTeam>, AttemptError> {
    let configured = rows
        .iter()
        .filter(|row| row.captain_user_id.is_some())
        .count();
    if rows.len() != usize::try_from(team_count).unwrap_or_default() {
        return Err(AttemptError::AuctionSetupTeamCountInvalid {
            expected: team_count,
            actual: rows.len(),
        });
    }
    if configured != rows.len() {
        return Err(AttemptError::AuctionSetupDraftIncomplete {
            configured: i16::try_from(configured).map_err(|_| AttemptError::InvariantViolation)?,
            required: team_count,
        });
    }
    let inputs = rows
        .iter()
        .map(|row| {
            Ok(AuctionSetupDraftTeamInput {
                team_index: row.team_index,
                team_name: row.team_name.clone(),
                captain_user_id: row
                    .captain_user_id
                    .clone()
                    .ok_or(AttemptError::InvariantViolation)?,
                initial_points: row.initial_points,
            })
        })
        .collect::<Result<Vec<_>, AttemptError>>()?;
    normalize_auction_setup_team_inputs(&inputs, team_count, joined)
}

fn has_complete_auction_setup(
    rows: &[AuctionSetupTeamRow],
    team_count: i16,
    target_count: i16,
    participants: &[AuctionRecruitmentParticipantRow],
) -> bool {
    if participants.len() != usize::try_from(target_count).unwrap_or_default()
        || rows.len() != usize::try_from(team_count).unwrap_or_default()
    {
        return false;
    }
    let joined_ids = participants
        .iter()
        .map(|participant| participant.id.clone())
        .collect::<HashSet<_>>();
    let mut captains = HashSet::new();
    rows.iter().enumerate().all(|(position, row)| {
        let expected_index = i16::try_from(position + 1).ok();
        let Some(captain) = row.captain_user_id.as_ref() else {
            return false;
        };
        expected_index == Some(row.team_index)
            && !row.team_name.trim().is_empty()
            && row.team_name.trim().len() <= MAX_AUCTION_TEAM_NAME_BYTES
            && row.initial_points > 0
            && joined_ids.contains(captain)
            && captains.insert(captain.clone())
    })
}

#[derive(Clone)]
struct AuctionDraftCurrentTarget {
    user_id: UserId,
    lot_id: Uuid,
}

async fn ensure_auction_draft_command_allowed(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    tournament: &AuctionTournamentLockRow,
    command: &AuctionTournamentDraftCommand,
) -> Result<(), AttemptError> {
    let has_bracket_artifacts =
        auction_tournament_has_bracket_artifacts(transaction, guild_id, tournament_id).await?;
    if has_bracket_artifacts {
        return Err(AttemptError::AuctionTournamentDraftLocked);
    }
    match tournament.status {
        AuctionTournamentStatus::Bidding => Ok(()),
        AuctionTournamentStatus::BracketSetup
            if matches!(
                command,
                AuctionTournamentDraftCommand::RevertLast { .. }
                    | AuctionTournamentDraftCommand::ResetToInitial { .. }
            ) && tournament.bracket_locked_at.is_none() =>
        {
            Ok(())
        }
        status => Err(AttemptError::AuctionTournamentDraftNotEditable(status)),
    }
}

async fn auction_tournament_has_bracket_artifacts(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<bool, AttemptError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM auction_matches
             WHERE guild_id = $1
               AND tournament_id = $2
               AND deleted_at IS NULL
               AND status <> 'CANCELLED'
             UNION ALL
             SELECT 1
             FROM auction_match_lineups AS lineup
             JOIN auction_matches AS auction_match
               ON auction_match.guild_id = lineup.guild_id
              AND auction_match.id = lineup.match_id
             WHERE lineup.guild_id = $1
               AND lineup.tournament_id = $2
               AND auction_match.deleted_at IS NULL
               AND auction_match.status <> 'CANCELLED'
             UNION ALL
             SELECT 1
             FROM games AS game
             JOIN auction_matches AS match
               ON match.guild_id = game.guild_id
              AND match.id = game.auction_match_id
             WHERE game.guild_id = $1
               AND match.tournament_id = $2
               AND match.deleted_at IS NULL
               AND match.status <> 'CANCELLED'
         )",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn auction_tournament_recent_unreversed_draft_action(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<Option<AuctionTournamentRecentActionSnapshot>, AttemptError> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT action.kind
         FROM auction_actions AS action
         WHERE action.guild_id = $1
           AND action.tournament_id = $2
           AND action.kind <> 'REVERT_LAST'
           AND NOT EXISTS (
               SELECT 1
               FROM auction_actions AS reversal
               WHERE reversal.guild_id = action.guild_id
                 AND reversal.tournament_id = action.tournament_id
                 AND reversal.kind = 'REVERT_LAST'
                 AND reversal.reverses_action_id = action.id
           )
         ORDER BY action.sequence DESC
         LIMIT 1",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .map(|kind| AuctionTournamentRecentActionSnapshot { kind }))
}

async fn auction_tournament_has_latest_revertible_draft_action(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<bool, AttemptError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM (
                 SELECT kind
                 FROM auction_actions
                 WHERE guild_id = $1 AND tournament_id = $2
                 ORDER BY sequence DESC, id DESC
                 LIMIT 1
             ) AS latest
             WHERE latest.kind IN (
                 'DRAW_NEXT',
                 'MARK_UNSOLD',
                 'AWARD_BID',
                 'ASSIGN_CURRENT',
                 'ASSIGN_UNSOLD',
                 'RESTORE_UNSOLD'
             )
         )",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

fn validate_auction_draft_state(
    candidates: &[AuctionTournamentCandidateLockRow],
    lots: &[AuctionLotLockRow],
    teams: &[AuctionTournamentTeamRow],
    members: &[AuctionTeamMemberLockRow],
) -> Result<(), AttemptError> {
    let _ = current_auction_draft_target(candidates, lots)?;
    let mut assigned_users = HashSet::new();
    let mut team_slots = HashSet::new();
    for member in members {
        if !assigned_users.insert(member.user_id.clone())
            || !team_slots.insert((member.team_id, member.roster_slot))
            || !(1..=5).contains(&member.roster_slot)
        {
            return Err(AttemptError::InvariantViolation);
        }
    }
    for team in teams {
        if team.current_points < 0 || team.point_revision < 0 {
            return Err(AttemptError::InvariantViolation);
        }
        if members
            .iter()
            .filter(|member| member.team_id == team.id)
            .count()
            > 5
        {
            return Err(AttemptError::InvariantViolation);
        }
    }
    for candidate in candidates {
        let is_member = assigned_users.contains(&candidate.user_id);
        match candidate.state.as_str() {
            "ASSIGNED" if !is_member => return Err(AttemptError::InvariantViolation),
            "QUEUED" | "ACTIVE" | "UNSOLD" if is_member => {
                return Err(AttemptError::InvariantViolation);
            }
            "QUEUED" | "ACTIVE" | "UNSOLD" | "ASSIGNED" => {}
            _ => return Err(AttemptError::InvariantViolation),
        }
    }
    Ok(())
}

fn current_auction_draft_target(
    candidates: &[AuctionTournamentCandidateLockRow],
    lots: &[AuctionLotLockRow],
) -> Result<Option<AuctionDraftCurrentTarget>, AttemptError> {
    let active_candidates = candidates
        .iter()
        .filter(|candidate| candidate.state == "ACTIVE")
        .collect::<Vec<_>>();
    let active_lots = lots
        .iter()
        .filter(|lot| lot.status == "ACTIVE")
        .collect::<Vec<_>>();
    match (active_candidates.as_slice(), active_lots.as_slice()) {
        ([], []) => Ok(None),
        ([candidate], [lot]) if candidate.user_id == lot.target_user_id => {
            Ok(Some(AuctionDraftCurrentTarget {
                user_id: candidate.user_id.clone(),
                lot_id: lot.id,
            }))
        }
        _ => Err(AttemptError::InvariantViolation),
    }
}

fn auction_candidate<'a>(
    candidates: &'a [AuctionTournamentCandidateLockRow],
    user_id: &UserId,
) -> Result<&'a AuctionTournamentCandidateLockRow, AttemptError> {
    candidates
        .iter()
        .find(|candidate| candidate.user_id == *user_id)
        .ok_or(AttemptError::AuctionDraftCandidateNotFound)
}

fn auction_team<'a>(
    teams: &'a [AuctionTournamentTeamRow],
    team_id: Uuid,
) -> Result<&'a AuctionTournamentTeamRow, AttemptError> {
    teams
        .iter()
        .find(|team| team.id == team_id)
        .ok_or(AttemptError::AuctionDraftTeamNotFound)
}

fn auction_unsold_lot<'a>(
    lots: &'a [AuctionLotLockRow],
    user_id: &UserId,
) -> Result<&'a AuctionLotLockRow, AttemptError> {
    lots.iter()
        .filter(|lot| lot.target_user_id == *user_id && lot.status == "UNSOLD")
        .max_by_key(|lot| lot.draw_sequence)
        .ok_or(AttemptError::AuctionDraftUnsoldLotNotFound)
}

fn auction_unsold_history<'a>(
    lots: &'a [AuctionLotLockRow],
    user_id: &UserId,
) -> Result<&'a AuctionLotLockRow, AttemptError> {
    lots.iter()
        .filter(|lot| {
            lot.target_user_id == *user_id && matches!(lot.status.as_str(), "UNSOLD" | "REVERTED")
        })
        .max_by_key(|lot| lot.draw_sequence)
        .ok_or(AttemptError::AuctionDraftUnsoldLotNotFound)
}

fn next_auction_action_sequence(actions: &[AuctionActionLockRow]) -> Result<i64, AttemptError> {
    actions
        .iter()
        .map(|action| action.sequence)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)
}

fn next_auction_lot_sequence(lots: &[AuctionLotLockRow]) -> Result<i64, AttemptError> {
    lots.iter()
        .map(|lot| lot.draw_sequence)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)
}

fn available_auction_roster_slot(
    members: &[AuctionTeamMemberLockRow],
    team_id: Uuid,
) -> Result<i16, AttemptError> {
    let occupied = members
        .iter()
        .filter(|member| member.team_id == team_id)
        .map(|member| member.roster_slot)
        .collect::<HashSet<_>>();
    (1..=5)
        .find(|slot| !occupied.contains(slot))
        .ok_or(AttemptError::AuctionDraftTeamFull)
}

fn all_auction_teams_full_after_add(
    teams: &[AuctionTournamentTeamRow],
    members: &[AuctionTeamMemberLockRow],
    added_team_id: Uuid,
) -> bool {
    !teams.is_empty()
        && teams.iter().all(|team| {
            let count = members
                .iter()
                .filter(|member| member.team_id == team.id)
                .count()
                + usize::from(team.id == added_team_id);
            count == 5
        })
}

async fn insert_auction_action(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    id: Uuid,
    sequence: i64,
    kind: &str,
    target_user_id: Option<&UserId>,
    team_id: Option<Uuid>,
    lot_id: Option<Uuid>,
    points: Option<i32>,
    reverses: Option<&AuctionActionLockRow>,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let reverses_action_id = reverses.map(|action| action.id);
    let reversed_action_kind = reverses.map(|action| action.kind.clone());
    sqlx::query(
        "INSERT INTO auction_actions (
             guild_id, tournament_id, id, sequence, kind, target_user_id, team_id, lot_id,
             points, reverses_action_id, reversed_action_kind, actor_type, actor_id, created_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'USER', $12, $13)",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(id)
    .bind(sequence)
    .bind(kind)
    .bind(target_user_id)
    .bind(team_id)
    .bind(lot_id)
    .bind(points)
    .bind(reverses_action_id)
    .bind(reversed_action_kind)
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn randomized_auction_candidate_user_ids(
    transaction: &mut DatabaseTransaction,
    source: &[UserId],
) -> Result<Vec<UserId>, AttemptError> {
    if source.len() <= 1 {
        return Ok(source.to_vec());
    }

    let source_ids = source
        .iter()
        .map(|user_id| user_id.as_str().to_owned())
        .collect::<Vec<_>>();
    let randomized_ids = sqlx::query_scalar::<_, String>(
        "SELECT requested.user_id
         FROM unnest($1::text[]) AS requested(user_id)
         ORDER BY random(), requested.user_id",
    )
    .bind(&source_ids)
    .fetch_all(&mut **transaction)
    .await?;
    let mut source_by_id = source
        .iter()
        .map(|user_id| (user_id.as_str().to_owned(), user_id.clone()))
        .collect::<HashMap<_, _>>();
    let mut randomized = randomized_ids
        .into_iter()
        .map(|user_id| {
            source_by_id
                .remove(&user_id)
                .ok_or(AttemptError::InvariantViolation)
        })
        .collect::<Result<Vec<_>, _>>()?;
    rotate_if_order_preserved(source, &mut randomized);
    Ok(randomized)
}

fn rotate_if_order_preserved<T: Eq>(source: &[T], order: &mut [T]) {
    if source.len() > 1 && source == order {
        order.rotate_left(1);
    }
}

async fn reshuffle_auction_queued_candidates(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    candidates: &[AuctionTournamentCandidateLockRow],
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let queued = candidates
        .iter()
        .filter(|candidate| candidate.state == "QUEUED")
        .collect::<Vec<_>>();
    if queued.len() <= 1 {
        return Ok(());
    }
    let source = queued
        .iter()
        .map(|candidate| candidate.user_id.clone())
        .collect::<Vec<_>>();
    let positions = queued
        .iter()
        .map(|candidate| candidate.queue_position)
        .collect::<Vec<_>>();
    let randomized = randomized_auction_candidate_user_ids(transaction, &source).await?;
    assign_auction_candidate_queue_positions(
        transaction,
        guild_id,
        tournament_id,
        &randomized,
        &positions,
        now,
        false,
    )
    .await
}

async fn reset_auction_tournament_draft_to_initial(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    candidates: &[AuctionTournamentCandidateLockRow],
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    clear_unlocked_auction_bracket_slots(transaction, guild_id, tournament_id, actor_id, now)
        .await?;
    sqlx::query(
        "DELETE FROM auction_point_ledger
         WHERE guild_id = $1 AND tournament_id = $2 AND reason = 'REVERSAL'",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "DELETE FROM auction_point_ledger
         WHERE guild_id = $1 AND tournament_id = $2 AND reason = 'BID_DEBIT'",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "DELETE FROM auction_team_members
         WHERE guild_id = $1 AND tournament_id = $2 AND acquired_via <> 'CAPTAIN'",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "DELETE FROM auction_actions
         WHERE guild_id = $1 AND tournament_id = $2 AND kind = 'REVERT_LAST'",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "DELETE FROM auction_actions
         WHERE guild_id = $1 AND tournament_id = $2",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "DELETE FROM auction_lots
         WHERE guild_id = $1 AND tournament_id = $2",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE auction_teams
         SET current_points = initial_points, point_revision = 0
         WHERE guild_id = $1 AND tournament_id = $2",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;

    let source = candidates
        .iter()
        .map(|candidate| candidate.user_id.clone())
        .collect::<Vec<_>>();
    let positions = candidates
        .iter()
        .map(|candidate| candidate.queue_position)
        .collect::<Vec<_>>();
    let randomized = randomized_auction_candidate_user_ids(transaction, &source).await?;
    assign_auction_candidate_queue_positions(
        transaction,
        guild_id,
        tournament_id,
        &randomized,
        &positions,
        now,
        true,
    )
    .await
}

async fn assign_auction_candidate_queue_positions(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    ordered_user_ids: &[UserId],
    queue_positions: &[i64],
    now: OffsetDateTime,
    reset_state: bool,
) -> Result<(), AttemptError> {
    if ordered_user_ids.is_empty() {
        return Ok(());
    }
    if ordered_user_ids.len() != queue_positions.len() {
        return Err(AttemptError::InvariantViolation);
    }
    let user_ids = ordered_user_ids
        .iter()
        .map(|user_id| user_id.as_str().to_owned())
        .collect::<Vec<_>>();
    let max_position = queue_positions
        .iter()
        .copied()
        .max()
        .ok_or(AttemptError::InvariantViolation)?;
    let temporary_offset = max_position
        .checked_add(
            i64::try_from(ordered_user_ids.len()).map_err(|_| AttemptError::InvariantViolation)?,
        )
        .and_then(|value| value.checked_add(1))
        .ok_or(AttemptError::InvariantViolation)?;
    let expected_rows =
        u64::try_from(ordered_user_ids.len()).map_err(|_| AttemptError::InvariantViolation)?;

    let bumped = sqlx::query(
        "UPDATE auction_bid_candidates
         SET queue_position = queue_position + $3
         WHERE guild_id = $1
           AND tournament_id = $2
           AND user_id = ANY($4::text[])",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(temporary_offset)
    .bind(&user_ids)
    .execute(&mut **transaction)
    .await?;
    if bumped.rows_affected() != expected_rows {
        return Err(AttemptError::InvariantViolation);
    }

    let query = if reset_state {
        "WITH assignment AS (
             SELECT requested.user_id, requested.queue_position
             FROM unnest($3::text[], $4::bigint[]) AS requested(user_id, queue_position)
         )
         UPDATE auction_bid_candidates AS candidate
         SET queue_position = assignment.queue_position,
             state = 'QUEUED',
             draw_count = 0,
             updated_at = $5
         FROM assignment
         WHERE candidate.guild_id = $1
           AND candidate.tournament_id = $2
           AND candidate.user_id = assignment.user_id"
    } else {
        "WITH assignment AS (
             SELECT requested.user_id, requested.queue_position
             FROM unnest($3::text[], $4::bigint[]) AS requested(user_id, queue_position)
         )
         UPDATE auction_bid_candidates AS candidate
         SET queue_position = assignment.queue_position,
             updated_at = $5
         FROM assignment
         WHERE candidate.guild_id = $1
           AND candidate.tournament_id = $2
           AND candidate.user_id = assignment.user_id"
    };
    let assigned = sqlx::query(query)
        .bind(guild_id)
        .bind(tournament_id.as_uuid())
        .bind(&user_ids)
        .bind(queue_positions)
        .bind(now)
        .execute(&mut **transaction)
        .await?;
    if assigned.rows_affected() == expected_rows {
        Ok(())
    } else {
        Err(AttemptError::InvariantViolation)
    }
}

async fn insert_auction_active_lot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    lot_id: Uuid,
    draw_sequence: i64,
    target_user_id: &UserId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "INSERT INTO auction_lots (
             guild_id, tournament_id, id, draw_sequence, target_user_id, status,
             opened_by_type, opened_by_id, opened_at, closed_by_type, closed_by_id, closed_at
         ) VALUES ($1, $2, $3, $4, $5, 'ACTIVE', 'USER', $6, $7, NULL, NULL, NULL)",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(lot_id)
    .bind(draw_sequence)
    .bind(target_user_id)
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn transition_auction_candidate(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    user_id: &UserId,
    expected_state: &str,
    next_state: &str,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let changed = sqlx::query(
        "UPDATE auction_bid_candidates
         SET state = $1, updated_at = $2
         WHERE guild_id = $3 AND tournament_id = $4 AND user_id = $5 AND state = $6",
    )
    .bind(next_state)
    .bind(now)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(user_id)
    .bind(expected_state)
    .execute(&mut **transaction)
    .await?;
    if changed.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AttemptError::AuctionDraftCandidateStateMismatch)
    }
}

async fn increment_auction_candidate_draw_count(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    user_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let changed = sqlx::query(
        "UPDATE auction_bid_candidates
         SET state = 'ACTIVE', draw_count = draw_count + 1, updated_at = $1
         WHERE guild_id = $2 AND tournament_id = $3 AND user_id = $4 AND state = 'QUEUED'",
    )
    .bind(now)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;
    if changed.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AttemptError::AuctionDraftCandidateStateMismatch)
    }
}

async fn close_active_auction_lot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    lot_id: Uuid,
    next_status: &str,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let changed = sqlx::query(
        "UPDATE auction_lots
         SET status = $1, closed_by_type = 'USER', closed_by_id = $2, closed_at = $3
         WHERE guild_id = $4 AND tournament_id = $5 AND id = $6 AND status = 'ACTIVE'",
    )
    .bind(next_status)
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(lot_id)
    .execute(&mut **transaction)
    .await?;
    if changed.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AttemptError::AuctionDraftCurrentTargetMismatch)
    }
}

async fn revert_closed_auction_lot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    lot_id: Uuid,
) -> Result<(), AttemptError> {
    let changed = sqlx::query(
        "UPDATE auction_lots
         SET status = 'REVERTED'
         WHERE guild_id = $1 AND tournament_id = $2 AND id = $3 AND status <> 'ACTIVE'",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(lot_id)
    .execute(&mut **transaction)
    .await?;
    if changed.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AttemptError::InvariantViolation)
    }
}

async fn insert_auction_team_member(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    member_id: Uuid,
    team_id: Uuid,
    roster_slot: i16,
    candidate: &AuctionTournamentCandidateLockRow,
    acquired_via: &str,
    acquisition_action_id: Uuid,
    acquisition_action_kind: &str,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "INSERT INTO auction_team_members (
             guild_id, tournament_id, team_id, id, user_id, roster_slot, acquired_via,
             declared_lane, mmr_at_acquisition, snapshot_origin, acquisition_action_id,
             acquisition_action_kind, joined_at, released_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, NULL)",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(team_id)
    .bind(member_id)
    .bind(&candidate.user_id)
    .bind(roster_slot)
    .bind(acquired_via)
    .bind(candidate.declared_lane)
    .bind(candidate.mmr_at_start)
    .bind(&candidate.snapshot_origin)
    .bind(acquisition_action_id)
    .bind(acquisition_action_kind)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn release_auction_team_member_for_action(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    action_id: Uuid,
    user_id: &UserId,
    now: OffsetDateTime,
) -> Result<AuctionTeamMemberLockRow, AttemptError> {
    let member = sqlx::query_as::<_, AuctionTeamMemberLockRow>(
        "SELECT id, team_id, user_id, roster_slot, acquisition_action_id, acquired_via
         FROM auction_team_members
         WHERE guild_id = $1
           AND tournament_id = $2
           AND acquisition_action_id = $3
           AND user_id = $4
           AND released_at IS NULL
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(action_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)?;
    let released = sqlx::query(
        "UPDATE auction_team_members
         SET released_at = $1
         WHERE guild_id = $2 AND tournament_id = $3 AND id = $4 AND released_at IS NULL",
    )
    .bind(now)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(member.id)
    .execute(&mut **transaction)
    .await?;
    if released.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }
    Ok(member)
}

async fn bump_auction_tournament_revision(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    status: AuctionTournamentStatus,
    now: OffsetDateTime,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE auction_tournaments
         SET status = $1, revision = revision + 1, updated_by = $2, updated_at = $3
         WHERE guild_id = $4 AND id = $5 AND deleted_at IS NULL
         RETURNING revision",
    )
    .bind(status)
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)
}

async fn update_auction_team_points(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    team: &AuctionTournamentTeamRow,
    points_after: i32,
) -> Result<i64, AttemptError> {
    let next_revision = team
        .point_revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;
    let changed = sqlx::query(
        "UPDATE auction_teams
         SET current_points = $1, point_revision = $2
         WHERE guild_id = $3
           AND tournament_id = $4
           AND id = $5
           AND current_points = $6
           AND point_revision = $7",
    )
    .bind(points_after)
    .bind(next_revision)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(team.id)
    .bind(team.current_points)
    .bind(team.point_revision)
    .execute(&mut **transaction)
    .await?;
    if changed.rows_affected() == 1 {
        Ok(next_revision)
    } else {
        Err(AttemptError::InvariantViolation)
    }
}

async fn insert_auction_bid_debit_ledger(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    ledger_id: Uuid,
    team: &AuctionTournamentTeamRow,
    point_revision: i64,
    points_after: i32,
    action_id: Uuid,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "INSERT INTO auction_point_ledger (
             guild_id, tournament_id, id, team_id, point_revision, reason,
             source_action_id, source_action_kind, points_before, points_after, delta,
             reverses_entry_id, reversed_reason, actor_type, actor_id, created_at
         ) VALUES ($1, $2, $3, $4, $5, 'BID_DEBIT', $6, 'AWARD_BID', $7, $8, $9,
                   NULL, NULL, 'USER', $10, $11)",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(ledger_id)
    .bind(team.id)
    .bind(point_revision)
    .bind(action_id)
    .bind(team.current_points)
    .bind(points_after)
    .bind(points_after - team.current_points)
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_auction_reversal_ledger(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    ledger_id: Uuid,
    team: &AuctionTournamentTeamRow,
    point_revision: i64,
    points_after: i32,
    reversal_action_id: Uuid,
    reversed_ledger: &AuctionPointLedgerLockRow,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "INSERT INTO auction_point_ledger (
             guild_id, tournament_id, id, team_id, point_revision, reason,
             source_action_id, source_action_kind, points_before, points_after, delta,
             reverses_entry_id, reversed_reason, actor_type, actor_id, created_at
         ) VALUES ($1, $2, $3, $4, $5, 'REVERSAL', $6, 'REVERT_LAST', $7, $8, $9,
                   $10, 'BID_DEBIT', 'USER', $11, $12)",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(ledger_id)
    .bind(team.id)
    .bind(point_revision)
    .bind(reversal_action_id)
    .bind(team.current_points)
    .bind(points_after)
    .bind(points_after - team.current_points)
    .bind(reversed_ledger.id)
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn apply_auction_tournament_draft_command(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    command: &AuctionTournamentDraftCommand,
    teams: &[AuctionTournamentTeamRow],
    members: &[AuctionTeamMemberLockRow],
    candidates: &[AuctionTournamentCandidateLockRow],
    lots: &[AuctionLotLockRow],
    actions: &[AuctionActionLockRow],
    point_ledger: &[AuctionPointLedgerLockRow],
    prepared: WebAuctionTournamentDraftPrepared,
    current_status: AuctionTournamentStatus,
) -> Result<AuctionTournamentStatus, AttemptError> {
    match command {
        AuctionTournamentDraftCommand::DrawNext { .. } => {
            if current_auction_draft_target(candidates, lots)?.is_some() {
                return Err(AttemptError::AuctionDraftCurrentTargetExists);
            }
            let candidate = candidates
                .iter()
                .filter(|candidate| candidate.state == "QUEUED")
                .min_by_key(|candidate| candidate.queue_position)
                .ok_or(AttemptError::AuctionDraftNoQueuedCandidate)?;
            let lot_sequence = next_auction_lot_sequence(lots)?;
            insert_auction_active_lot(
                transaction,
                guild_id,
                tournament_id,
                prepared.lot_id,
                lot_sequence,
                &candidate.user_id,
                actor_id,
                prepared.now,
            )
            .await?;
            increment_auction_candidate_draw_count(
                transaction,
                guild_id,
                tournament_id,
                &candidate.user_id,
                prepared.now,
            )
            .await?;
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                next_auction_action_sequence(actions)?,
                "DRAW_NEXT",
                Some(&candidate.user_id),
                None,
                Some(prepared.lot_id),
                None,
                None,
                actor_id,
                prepared.now,
            )
            .await?;
            Ok(AuctionTournamentStatus::Bidding)
        }
        AuctionTournamentDraftCommand::ReshuffleCandidates { .. } => {
            reshuffle_auction_queued_candidates(
                transaction,
                guild_id,
                tournament_id,
                candidates,
                prepared.now,
            )
            .await?;
            Ok(AuctionTournamentStatus::Bidding)
        }
        AuctionTournamentDraftCommand::MarkUnsold { member_id, .. } => {
            let target = current_auction_draft_target(candidates, lots)?
                .ok_or(AttemptError::AuctionDraftCurrentTargetRequired)?;
            if target.user_id != *member_id {
                return Err(AttemptError::AuctionDraftCurrentTargetMismatch);
            }
            let candidate = auction_candidate(candidates, member_id)?;
            if candidate.state != "ACTIVE" {
                return Err(AttemptError::AuctionDraftCandidateStateMismatch);
            }
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                next_auction_action_sequence(actions)?,
                "MARK_UNSOLD",
                Some(member_id),
                None,
                Some(target.lot_id),
                None,
                None,
                actor_id,
                prepared.now,
            )
            .await?;
            close_active_auction_lot(
                transaction,
                guild_id,
                tournament_id,
                target.lot_id,
                "UNSOLD",
                actor_id,
                prepared.now,
            )
            .await?;
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                &candidate.user_id,
                "ACTIVE",
                "UNSOLD",
                prepared.now,
            )
            .await?;
            Ok(AuctionTournamentStatus::Bidding)
        }
        AuctionTournamentDraftCommand::AwardBid {
            member_id,
            team_id,
            points,
            ..
        } => {
            let target = current_auction_draft_target(candidates, lots)?
                .ok_or(AttemptError::AuctionDraftCurrentTargetRequired)?;
            if target.user_id != *member_id {
                return Err(AttemptError::AuctionDraftCurrentTargetMismatch);
            }
            let candidate = auction_candidate(candidates, member_id)?;
            let team = auction_team(teams, *team_id)?;
            let slot = available_auction_roster_slot(members, *team_id)?;
            if *points < 0 || *points > team.current_points {
                return Err(AttemptError::AuctionDraftBidPointsInvalid {
                    current_points: team.current_points,
                    submitted_points: *points,
                });
            }
            let points_after = team.current_points.checked_sub(*points).ok_or(
                AttemptError::AuctionDraftBidPointsInvalid {
                    current_points: team.current_points,
                    submitted_points: *points,
                },
            )?;
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                next_auction_action_sequence(actions)?,
                "AWARD_BID",
                Some(member_id),
                Some(*team_id),
                Some(target.lot_id),
                Some(*points),
                None,
                actor_id,
                prepared.now,
            )
            .await?;
            insert_auction_team_member(
                transaction,
                guild_id,
                tournament_id,
                prepared.team_member_id,
                *team_id,
                slot,
                candidate,
                "BID",
                prepared.action_id,
                "AWARD_BID",
                prepared.now,
            )
            .await?;
            close_active_auction_lot(
                transaction,
                guild_id,
                tournament_id,
                target.lot_id,
                "AWARDED",
                actor_id,
                prepared.now,
            )
            .await?;
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                member_id,
                "ACTIVE",
                "ASSIGNED",
                prepared.now,
            )
            .await?;
            let point_revision = update_auction_team_points(
                transaction,
                guild_id,
                tournament_id,
                team,
                points_after,
            )
            .await?;
            insert_auction_bid_debit_ledger(
                transaction,
                guild_id,
                tournament_id,
                prepared.point_ledger_id,
                team,
                point_revision,
                points_after,
                prepared.action_id,
                actor_id,
                prepared.now,
            )
            .await?;
            Ok(
                if all_auction_teams_full_after_add(teams, members, *team_id) {
                    AuctionTournamentStatus::BracketSetup
                } else {
                    AuctionTournamentStatus::Bidding
                },
            )
        }
        AuctionTournamentDraftCommand::AssignCurrent {
            member_id, team_id, ..
        } => {
            let target = current_auction_draft_target(candidates, lots)?
                .ok_or(AttemptError::AuctionDraftCurrentTargetRequired)?;
            if target.user_id != *member_id {
                return Err(AttemptError::AuctionDraftCurrentTargetMismatch);
            }
            let candidate = auction_candidate(candidates, member_id)?;
            let _team = auction_team(teams, *team_id)?;
            let slot = available_auction_roster_slot(members, *team_id)?;
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                next_auction_action_sequence(actions)?,
                "ASSIGN_CURRENT",
                Some(member_id),
                Some(*team_id),
                Some(target.lot_id),
                None,
                None,
                actor_id,
                prepared.now,
            )
            .await?;
            insert_auction_team_member(
                transaction,
                guild_id,
                tournament_id,
                prepared.team_member_id,
                *team_id,
                slot,
                candidate,
                "MANUAL",
                prepared.action_id,
                "ASSIGN_CURRENT",
                prepared.now,
            )
            .await?;
            close_active_auction_lot(
                transaction,
                guild_id,
                tournament_id,
                target.lot_id,
                "ASSIGNED",
                actor_id,
                prepared.now,
            )
            .await?;
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                member_id,
                "ACTIVE",
                "ASSIGNED",
                prepared.now,
            )
            .await?;
            Ok(
                if all_auction_teams_full_after_add(teams, members, *team_id) {
                    AuctionTournamentStatus::BracketSetup
                } else {
                    AuctionTournamentStatus::Bidding
                },
            )
        }
        AuctionTournamentDraftCommand::AssignUnsold {
            member_id, team_id, ..
        } => {
            if current_auction_draft_target(candidates, lots)?.is_some() {
                return Err(AttemptError::AuctionDraftCurrentTargetExists);
            }
            let candidate = auction_candidate(candidates, member_id)?;
            if candidate.state != "UNSOLD" {
                return Err(AttemptError::AuctionDraftCandidateStateMismatch);
            }
            let lot = auction_unsold_lot(lots, member_id)?;
            let _team = auction_team(teams, *team_id)?;
            let slot = available_auction_roster_slot(members, *team_id)?;
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                next_auction_action_sequence(actions)?,
                "ASSIGN_UNSOLD",
                Some(member_id),
                Some(*team_id),
                Some(lot.id),
                None,
                None,
                actor_id,
                prepared.now,
            )
            .await?;
            insert_auction_team_member(
                transaction,
                guild_id,
                tournament_id,
                prepared.team_member_id,
                *team_id,
                slot,
                candidate,
                "MANUAL",
                prepared.action_id,
                "ASSIGN_UNSOLD",
                prepared.now,
            )
            .await?;
            let assigned = sqlx::query(
                "UPDATE auction_lots
                 SET status = 'ASSIGNED'
                 WHERE guild_id = $1 AND tournament_id = $2 AND id = $3 AND status = 'UNSOLD'",
            )
            .bind(guild_id)
            .bind(tournament_id.as_uuid())
            .bind(lot.id)
            .execute(&mut **transaction)
            .await?;
            if assigned.rows_affected() != 1 {
                return Err(AttemptError::AuctionDraftCandidateStateMismatch);
            }
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                member_id,
                "UNSOLD",
                "ASSIGNED",
                prepared.now,
            )
            .await?;
            Ok(
                if all_auction_teams_full_after_add(teams, members, *team_id) {
                    AuctionTournamentStatus::BracketSetup
                } else {
                    AuctionTournamentStatus::Bidding
                },
            )
        }
        AuctionTournamentDraftCommand::RestoreUnsold { member_id, .. } => {
            if current_auction_draft_target(candidates, lots)?.is_some() {
                return Err(AttemptError::AuctionDraftCurrentTargetExists);
            }
            let candidate = auction_candidate(candidates, member_id)?;
            if candidate.state != "UNSOLD" {
                return Err(AttemptError::AuctionDraftCandidateStateMismatch);
            }
            // A reverted manual assignment leaves the candidate UNSOLD with a historical
            // REVERTED lot. Restoring it still creates a new active lot and is not a re-draw.
            let _old_lot = auction_unsold_history(lots, member_id)?;
            let lot_sequence = next_auction_lot_sequence(lots)?;
            insert_auction_active_lot(
                transaction,
                guild_id,
                tournament_id,
                prepared.lot_id,
                lot_sequence,
                member_id,
                actor_id,
                prepared.now,
            )
            .await?;
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                next_auction_action_sequence(actions)?,
                "RESTORE_UNSOLD",
                Some(member_id),
                None,
                Some(prepared.lot_id),
                None,
                None,
                actor_id,
                prepared.now,
            )
            .await?;
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                member_id,
                "UNSOLD",
                "ACTIVE",
                prepared.now,
            )
            .await?;
            Ok(AuctionTournamentStatus::Bidding)
        }
        AuctionTournamentDraftCommand::ResetToInitial { .. } => {
            reset_auction_tournament_draft_to_initial(
                transaction,
                guild_id,
                tournament_id,
                actor_id,
                candidates,
                prepared.now,
            )
            .await?;
            Ok(AuctionTournamentStatus::Bidding)
        }
        AuctionTournamentDraftCommand::RevertLast { .. } => {
            let reversed_ids = actions
                .iter()
                .filter(|action| action.kind == "REVERT_LAST")
                .filter_map(|action| action.reverses_action_id)
                .collect::<HashSet<_>>();
            let original = actions
                .iter()
                .rev()
                .find(|action| action.kind != "REVERT_LAST" && !reversed_ids.contains(&action.id))
                .ok_or(AttemptError::AuctionDraftNoRevertableAction)?;
            if current_status == AuctionTournamentStatus::BracketSetup {
                clear_unlocked_auction_bracket_slots(
                    transaction,
                    guild_id,
                    tournament_id,
                    actor_id,
                    prepared.now,
                )
                .await?;
            }
            revert_auction_tournament_draft_action(
                transaction,
                guild_id,
                tournament_id,
                actor_id,
                original,
                teams,
                candidates,
                lots,
                actions,
                point_ledger,
                prepared,
            )
            .await?;
            let _ = current_status;
            Ok(AuctionTournamentStatus::Bidding)
        }
    }
}

async fn clear_unlocked_auction_bracket_slots(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "UPDATE auction_bracket_slots
         SET team_id = NULL, updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND tournament_id = $4 AND team_id IS NOT NULL",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn revert_auction_tournament_draft_action(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    original: &AuctionActionLockRow,
    teams: &[AuctionTournamentTeamRow],
    candidates: &[AuctionTournamentCandidateLockRow],
    lots: &[AuctionLotLockRow],
    actions: &[AuctionActionLockRow],
    point_ledger: &[AuctionPointLedgerLockRow],
    prepared: WebAuctionTournamentDraftPrepared,
) -> Result<(), AttemptError> {
    let target_user_id = original
        .target_user_id
        .as_ref()
        .ok_or(AttemptError::InvariantViolation)?;
    let action_sequence = next_auction_action_sequence(actions)?;
    let candidate = auction_candidate(candidates, target_user_id)?;
    let original_lot_id = original.lot_id;
    let current_target = current_auction_draft_target(candidates, lots)?;
    match original.kind.as_str() {
        "DRAW_NEXT" => {
            let lot_id = original_lot_id.ok_or(AttemptError::InvariantViolation)?;
            if current_target
                .as_ref()
                .map(|target| (&target.user_id, target.lot_id))
                != Some((target_user_id, lot_id))
                || candidate.state != "ACTIVE"
            {
                return Err(AttemptError::AuctionDraftCurrentTargetMismatch);
            }
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                action_sequence,
                "REVERT_LAST",
                Some(target_user_id),
                None,
                None,
                None,
                Some(original),
                actor_id,
                prepared.now,
            )
            .await?;
            close_active_auction_lot(
                transaction,
                guild_id,
                tournament_id,
                lot_id,
                "REVERTED",
                actor_id,
                prepared.now,
            )
            .await?;
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                target_user_id,
                "ACTIVE",
                "QUEUED",
                prepared.now,
            )
            .await?;
        }
        "MARK_UNSOLD" => {
            let lot_id = original_lot_id.ok_or(AttemptError::InvariantViolation)?;
            if current_target.is_some() || candidate.state != "UNSOLD" {
                return Err(AttemptError::AuctionDraftCandidateStateMismatch);
            }
            let old_lot = lots
                .iter()
                .find(|lot| lot.id == lot_id)
                .ok_or(AttemptError::InvariantViolation)?;
            if old_lot.status != "UNSOLD" {
                return Err(AttemptError::InvariantViolation);
            }
            insert_auction_active_lot(
                transaction,
                guild_id,
                tournament_id,
                prepared.lot_id,
                next_auction_lot_sequence(lots)?,
                target_user_id,
                actor_id,
                prepared.now,
            )
            .await?;
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                action_sequence,
                "REVERT_LAST",
                Some(target_user_id),
                None,
                Some(prepared.lot_id),
                None,
                Some(original),
                actor_id,
                prepared.now,
            )
            .await?;
            revert_closed_auction_lot(transaction, guild_id, tournament_id, lot_id).await?;
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                target_user_id,
                "UNSOLD",
                "ACTIVE",
                prepared.now,
            )
            .await?;
        }
        "AWARD_BID" => {
            let lot_id = original_lot_id.ok_or(AttemptError::InvariantViolation)?;
            let team_id = original.team_id.ok_or(AttemptError::InvariantViolation)?;
            let team = auction_team(teams, team_id)?;
            let debit = point_ledger
                .iter()
                .find(|ledger| {
                    ledger.team_id == team_id
                        && ledger.reason == "BID_DEBIT"
                        && ledger.source_action_id == Some(original.id)
                })
                .ok_or(AttemptError::InvariantViolation)?;
            if current_target.is_some()
                || candidate.state != "ASSIGNED"
                || team.point_revision != debit.point_revision
                || team.current_points != debit.points_after
                || original.points != Some(debit.points_before - debit.points_after)
            {
                return Err(AttemptError::AuctionDraftRevertUnavailable);
            }
            insert_auction_active_lot(
                transaction,
                guild_id,
                tournament_id,
                prepared.lot_id,
                next_auction_lot_sequence(lots)?,
                target_user_id,
                actor_id,
                prepared.now,
            )
            .await?;
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                action_sequence,
                "REVERT_LAST",
                Some(target_user_id),
                Some(team_id),
                Some(prepared.lot_id),
                None,
                Some(original),
                actor_id,
                prepared.now,
            )
            .await?;
            let released = release_auction_team_member_for_action(
                transaction,
                guild_id,
                tournament_id,
                original.id,
                target_user_id,
                prepared.now,
            )
            .await?;
            if released.team_id != team_id || released.acquired_via != "BID" {
                return Err(AttemptError::InvariantViolation);
            }
            revert_closed_auction_lot(transaction, guild_id, tournament_id, lot_id).await?;
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                target_user_id,
                "ASSIGNED",
                "ACTIVE",
                prepared.now,
            )
            .await?;
            let point_revision = update_auction_team_points(
                transaction,
                guild_id,
                tournament_id,
                team,
                debit.points_before,
            )
            .await?;
            insert_auction_reversal_ledger(
                transaction,
                guild_id,
                tournament_id,
                prepared.point_ledger_id,
                team,
                point_revision,
                debit.points_before,
                prepared.action_id,
                debit,
                actor_id,
                prepared.now,
            )
            .await?;
        }
        "ASSIGN_CURRENT" => {
            let lot_id = original_lot_id.ok_or(AttemptError::InvariantViolation)?;
            let team_id = original.team_id.ok_or(AttemptError::InvariantViolation)?;
            if current_target.is_some() || candidate.state != "ASSIGNED" {
                return Err(AttemptError::AuctionDraftCandidateStateMismatch);
            }
            insert_auction_active_lot(
                transaction,
                guild_id,
                tournament_id,
                prepared.lot_id,
                next_auction_lot_sequence(lots)?,
                target_user_id,
                actor_id,
                prepared.now,
            )
            .await?;
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                action_sequence,
                "REVERT_LAST",
                Some(target_user_id),
                Some(team_id),
                Some(prepared.lot_id),
                None,
                Some(original),
                actor_id,
                prepared.now,
            )
            .await?;
            let released = release_auction_team_member_for_action(
                transaction,
                guild_id,
                tournament_id,
                original.id,
                target_user_id,
                prepared.now,
            )
            .await?;
            if released.team_id != team_id || released.acquired_via != "MANUAL" {
                return Err(AttemptError::InvariantViolation);
            }
            revert_closed_auction_lot(transaction, guild_id, tournament_id, lot_id).await?;
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                target_user_id,
                "ASSIGNED",
                "ACTIVE",
                prepared.now,
            )
            .await?;
        }
        "ASSIGN_UNSOLD" => {
            let lot_id = original_lot_id.ok_or(AttemptError::InvariantViolation)?;
            let team_id = original.team_id.ok_or(AttemptError::InvariantViolation)?;
            if current_target.is_some() || candidate.state != "ASSIGNED" {
                return Err(AttemptError::AuctionDraftCandidateStateMismatch);
            }
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                action_sequence,
                "REVERT_LAST",
                Some(target_user_id),
                Some(team_id),
                None,
                None,
                Some(original),
                actor_id,
                prepared.now,
            )
            .await?;
            let released = release_auction_team_member_for_action(
                transaction,
                guild_id,
                tournament_id,
                original.id,
                target_user_id,
                prepared.now,
            )
            .await?;
            if released.team_id != team_id || released.acquired_via != "MANUAL" {
                return Err(AttemptError::InvariantViolation);
            }
            revert_closed_auction_lot(transaction, guild_id, tournament_id, lot_id).await?;
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                target_user_id,
                "ASSIGNED",
                "UNSOLD",
                prepared.now,
            )
            .await?;
        }
        "RESTORE_UNSOLD" => {
            let lot_id = original_lot_id.ok_or(AttemptError::InvariantViolation)?;
            if current_target
                .as_ref()
                .map(|target| (&target.user_id, target.lot_id))
                != Some((target_user_id, lot_id))
                || candidate.state != "ACTIVE"
            {
                return Err(AttemptError::AuctionDraftCurrentTargetMismatch);
            }
            insert_auction_action(
                transaction,
                guild_id,
                tournament_id,
                prepared.action_id,
                action_sequence,
                "REVERT_LAST",
                Some(target_user_id),
                None,
                None,
                None,
                Some(original),
                actor_id,
                prepared.now,
            )
            .await?;
            close_active_auction_lot(
                transaction,
                guild_id,
                tournament_id,
                lot_id,
                "REVERTED",
                actor_id,
                prepared.now,
            )
            .await?;
            transition_auction_candidate(
                transaction,
                guild_id,
                tournament_id,
                target_user_id,
                "ACTIVE",
                "UNSOLD",
                prepared.now,
            )
            .await?;
        }
        _ => return Err(AttemptError::InvariantViolation),
    }
    Ok(())
}

fn is_auction_bracket_slot_key(slot_key: &str) -> bool {
    AUCTION_BRACKET_SLOT_KEYS.contains(&slot_key)
}

fn is_auction_match_key(match_key: &str) -> bool {
    matches!(match_key, "SEMIFINAL_A" | "SEMIFINAL_B" | "FINAL")
}

fn is_auction_semifinal_match_key(match_key: &str) -> bool {
    matches!(match_key, "SEMIFINAL_A" | "SEMIFINAL_B")
}

fn requires_twenty_person_semifinal_lineup_gate(tournament_format: &str, match_key: &str) -> bool {
    tournament_format == "TWENTY_PERSON" && is_auction_semifinal_match_key(match_key)
}

fn can_enter_auction_match_results(
    is_operator: bool,
    lineups_locked: bool,
    semifinal_lineups_ready: bool,
    status: &str,
) -> bool {
    is_operator
        && lineups_locked
        && semifinal_lineups_ready
        && matches!(status, "READY" | "IN_PROGRESS")
}

fn can_revert_auction_match_last_game(
    is_operator: bool,
    lineups_locked: bool,
    has_confirmed_games: bool,
    status: &str,
    downstream_final_blocks_revert: bool,
) -> bool {
    is_operator
        && lineups_locked
        && has_confirmed_games
        && !downstream_final_blocks_revert
        && matches!(status, "READY" | "IN_PROGRESS" | "COMPLETED")
}

fn is_auction_match_status(status: &str) -> bool {
    matches!(
        status,
        "LINEUP_PENDING" | "READY" | "IN_PROGRESS" | "COMPLETED" | "CANCELLED"
    )
}

fn auction_match_label(match_key: &str) -> Result<&'static str, AttemptError> {
    match match_key {
        "SEMIFINAL_A" => Ok("준결승 A"),
        "SEMIFINAL_B" => Ok("준결승 B"),
        "FINAL" => Ok("결승"),
        _ => Err(AttemptError::InvariantViolation),
    }
}

fn auction_match_lineup_teams(
    auction_match: &AuctionMatchLockRow,
) -> Result<(Uuid, Uuid), AttemptError> {
    if !is_auction_match_status(&auction_match.status) {
        return Err(AttemptError::InvariantViolation);
    }
    let left_team_id = auction_match
        .left_team_id
        .ok_or(AttemptError::AuctionMatchTeamsPending)?;
    let right_team_id = auction_match
        .right_team_id
        .ok_or(AttemptError::AuctionMatchTeamsPending)?;
    if left_team_id == right_team_id {
        return Err(AttemptError::InvariantViolation);
    }
    Ok((left_team_id, right_team_id))
}

fn ensure_auction_match_lineup_editable(
    auction_match: &AuctionMatchLockRow,
) -> Result<(Uuid, Uuid), AttemptError> {
    let teams = auction_match_lineup_teams(auction_match)?;
    if auction_match.status != "LINEUP_PENDING" {
        return Err(AttemptError::AuctionMatchLineupNotEditable(
            auction_match.status.clone(),
        ));
    }
    if auction_match.lineups_locked_at.is_some() {
        return Err(AttemptError::AuctionMatchLineupLocked);
    }
    Ok(teams)
}

fn ensure_auction_match_lineup_unlockable(
    auction_match: &AuctionMatchLockRow,
) -> Result<(), AttemptError> {
    if auction_match.status != "READY"
        || auction_match.lineups_locked_at.is_none()
        || auction_match.lineups_locked_by.is_none()
    {
        return Err(AttemptError::AuctionMatchLineupUnlockUnavailable(
            auction_match.status.clone(),
        ));
    }
    Ok(())
}

async fn auction_match_has_result_artifacts(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
) -> Result<bool, AttemptError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM games
             WHERE guild_id = $1 AND auction_match_id = $2
         )
         OR EXISTS (
             SELECT 1
             FROM game_inputs
             WHERE guild_id = $1 AND auction_match_id = $2
         )",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn auction_completed_twenty_person_semifinal_revert_is_blocked(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    semifinal_match_key: &str,
    winner_team_id: Option<Uuid>,
) -> Result<bool, AttemptError> {
    if !is_auction_semifinal_match_key(semifinal_match_key) {
        return Ok(false);
    }
    let Some(winner_team_id) = winner_team_id else {
        return Ok(true);
    };
    let tournament_format = sqlx::query_scalar::<_, String>(
        "SELECT format
         FROM auction_tournaments
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)?;
    if parse_auction_format(&tournament_format)? != AuctionFormat::TwentyPerson {
        return Ok(false);
    }

    let final_match = sqlx::query_as::<_, AuctionFinalMatchRevertRow>(
        "SELECT id, status, left_team_id, right_team_id, lineups_locked_at
         FROM auction_matches
         WHERE guild_id = $1
           AND tournament_id = $2
           AND match_key = 'FINAL'
           AND status <> 'CANCELLED'
           AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(final_match) = final_match else {
        return Ok(true);
    };
    let target_team_id = match semifinal_match_key {
        "SEMIFINAL_A" => final_match.left_team_id,
        "SEMIFINAL_B" => final_match.right_team_id,
        _ => return Ok(true),
    };
    let Some(target_team_id) = target_team_id else {
        return Ok(true);
    };
    if target_team_id != winner_team_id {
        return Ok(true);
    }

    auction_final_has_downstream_work(
        transaction,
        guild_id,
        tournament_id,
        AuctionMatchId::from_uuid(final_match.id),
        target_team_id,
        final_match.lineups_locked_at,
        &final_match.status,
    )
    .await
}

async fn auction_final_has_downstream_work(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    final_match_id: AuctionMatchId,
    target_team_id: Uuid,
    final_lineups_locked_at: Option<OffsetDateTime>,
    final_status: &str,
) -> Result<bool, AttemptError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM auction_match_lineups
             WHERE guild_id = $1 AND tournament_id = $2 AND match_id = $3
               AND team_id <> $4
         )
         OR EXISTS (
             SELECT 1 FROM games
             WHERE guild_id = $1 AND auction_match_id = $3
         )
         OR $5 IS NOT NULL
         OR $6 <> 'LINEUP_PENDING'",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(final_match_id.as_uuid())
    .bind(target_team_id)
    .bind(final_lineups_locked_at)
    .bind(final_status)
    .fetch_one(&mut **transaction)
    .await?)
}

async fn ensure_auction_match_has_no_result_artifacts(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
) -> Result<(), AttemptError> {
    if auction_match_has_result_artifacts(transaction, guild_id, match_id).await? {
        Err(AttemptError::AuctionMatchLineupResultArtifactsPresent)
    } else {
        Ok(())
    }
}

async fn twenty_person_semifinal_lineups_are_locked(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<bool, AttemptError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT CASE
                    WHEN tournament.format <> 'TWENTY_PERSON' THEN TRUE
                    ELSE (
                        SELECT count(*) = 2
                            AND count(*) FILTER (
                                WHERE auction_match.lineups_locked_at IS NOT NULL
                            ) = 2
                        FROM auction_matches AS auction_match
                        WHERE auction_match.guild_id = tournament.guild_id
                          AND auction_match.tournament_id = tournament.id
                          AND auction_match.match_key IN ('SEMIFINAL_A', 'SEMIFINAL_B')
                          AND auction_match.status <> 'CANCELLED'
                          AND auction_match.deleted_at IS NULL
                    )
                END
         FROM auction_tournaments AS tournament
         WHERE tournament.guild_id = $1
           AND tournament.id = $2
           AND tournament.deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)
}

async fn ensure_twenty_person_semifinal_lineups_locked(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    tournament_format: &str,
    auction_match: &AuctionMatchLockRow,
) -> Result<(), AttemptError> {
    if !requires_twenty_person_semifinal_lineup_gate(tournament_format, &auction_match.match_key) {
        return Ok(());
    }

    // The tournament row is already locked by the caller. Lock both semifinal
    // rows before evaluating their state so the submit guard is authoritative.
    lock_auction_tournament_matches(transaction, guild_id, tournament_id).await?;
    if twenty_person_semifinal_lineups_are_locked(transaction, guild_id, tournament_id).await? {
        Ok(())
    } else {
        Err(AttemptError::AuctionMatchSemifinalLineupsIncomplete)
    }
}

fn has_complete_auction_match_lineup(
    lineups: &[AuctionMatchLineupAssignmentSnapshot],
    left_team_id: Uuid,
    right_team_id: Uuid,
) -> bool {
    if lineups.len() != 10 {
        return false;
    }
    let expected = [left_team_id, right_team_id]
        .into_iter()
        .flat_map(|team_id| {
            [
                Lane::Top,
                Lane::Jungle,
                Lane::Mid,
                Lane::Bottom,
                Lane::Support,
            ]
            .into_iter()
            .map(move |lane| (team_id, lane))
        })
        .collect::<HashSet<_>>();
    let actual = lineups
        .iter()
        .map(|lineup| (lineup.team_id, lineup.lane))
        .collect::<HashSet<_>>();
    expected == actual
        && lineups
            .iter()
            .map(|lineup| lineup.team_member_id)
            .collect::<HashSet<_>>()
            .len()
            == lineups.len()
        && lineups
            .iter()
            .map(|lineup| lineup.user_id.clone())
            .collect::<HashSet<_>>()
            .len()
            == lineups.len()
}

fn validate_auction_match_lineup_rows(
    lineups: &[AuctionMatchLineupRow],
    members: &[AuctionTeamMemberLockRow],
    left_team_id: Uuid,
    right_team_id: Uuid,
    require_complete: bool,
) -> Result<(), AttemptError> {
    let team_ids = HashSet::from([left_team_id, right_team_id]);
    let mut member_users = HashMap::<(Uuid, Uuid), &UserId>::new();
    let mut roster_users = HashSet::new();
    let mut roster_slots = HashSet::new();
    for member in members
        .iter()
        .filter(|member| team_ids.contains(&member.team_id))
    {
        if !(1..=5).contains(&member.roster_slot)
            || !roster_users.insert(member.user_id.clone())
            || !roster_slots.insert((member.team_id, member.roster_slot))
            || member_users
                .insert((member.team_id, member.id), &member.user_id)
                .is_some()
        {
            return Err(AttemptError::InvariantViolation);
        }
    }
    if require_complete
        && [left_team_id, right_team_id].into_iter().any(|team_id| {
            members
                .iter()
                .filter(|member| member.team_id == team_id)
                .count()
                != 5
        })
    {
        return Err(AttemptError::AuctionMatchLineupIncomplete);
    }
    let mut slots = HashSet::new();
    let mut assigned_members = HashSet::new();
    let mut assigned_users = HashSet::new();
    for lineup in lineups {
        if !team_ids.contains(&lineup.team_id)
            || !slots.insert((lineup.team_id, lineup.lane))
            || !assigned_members.insert(lineup.team_member_id)
            || !assigned_users.insert(lineup.user_id.clone())
            || member_users.get(&(lineup.team_id, lineup.team_member_id)) != Some(&&lineup.user_id)
        {
            return Err(AttemptError::InvariantViolation);
        }
    }
    if require_complete {
        let snapshot = lineups
            .iter()
            .map(|lineup| AuctionMatchLineupAssignmentSnapshot {
                team_id: lineup.team_id,
                lane: lineup.lane,
                team_member_id: lineup.team_member_id,
                user_id: lineup.user_id.clone(),
            })
            .collect::<Vec<_>>();
        if !has_complete_auction_match_lineup(&snapshot, left_team_id, right_team_id) {
            return Err(AttemptError::AuctionMatchLineupIncomplete);
        }
    }
    Ok(())
}

fn allowed_auction_bracket_slot_keys(format: AuctionFormat) -> &'static [&'static str] {
    match format {
        AuctionFormat::TenPerson => &AUCTION_BRACKET_SLOT_KEYS[..2],
        AuctionFormat::TwentyPerson => &AUCTION_BRACKET_SLOT_KEYS,
    }
}

fn ensure_auction_bracket_command_allowed(
    tournament: &AuctionTournamentBracketLockRow,
    matches: &[Uuid],
    command: &AuctionTournamentBracketCommand,
) -> Result<(), AttemptError> {
    if matches!(
        command,
        AuctionTournamentBracketCommand::UnlockBracket { .. }
    ) {
        if !matches!(
            tournament.status,
            AuctionTournamentStatus::BracketSetup | AuctionTournamentStatus::InGame
        ) || tournament.bracket_locked_at.is_none()
            || matches.is_empty()
        {
            return Err(AttemptError::AuctionTournamentBracketRecoveryUnavailable);
        }
        return Ok(());
    }
    if tournament.status != AuctionTournamentStatus::BracketSetup {
        return Err(AttemptError::AuctionTournamentBracketNotEditable(
            tournament.status,
        ));
    }
    if tournament.bracket_locked_at.is_some() || !matches.is_empty() {
        return Err(AttemptError::AuctionTournamentBracketLocked);
    }
    Ok(())
}

fn validate_auction_bracket_slots(
    slots: &[AuctionTournamentBracketSlotRow],
) -> Result<(), AttemptError> {
    if slots.len() != AUCTION_BRACKET_SLOT_KEYS.len() {
        return Err(AttemptError::InvariantViolation);
    }
    let mut slot_keys = HashSet::new();
    let mut team_ids = HashSet::new();
    for slot in slots {
        if !is_auction_bracket_slot_key(&slot.slot_key)
            || !slot_keys.insert(slot.slot_key.as_str())
            || slot
                .team_id
                .is_some_and(|team_id| !team_ids.insert(team_id))
        {
            return Err(AttemptError::InvariantViolation);
        }
    }
    if slot_keys.len() != AUCTION_BRACKET_SLOT_KEYS.len() {
        return Err(AttemptError::InvariantViolation);
    }
    Ok(())
}

fn auction_bracket_slot<'a>(
    slots: &'a [AuctionTournamentBracketSlotRow],
    slot_key: &str,
) -> Result<&'a AuctionTournamentBracketSlotRow, AttemptError> {
    slots
        .iter()
        .find(|slot| slot.slot_key == slot_key)
        .ok_or(AttemptError::InvariantViolation)
}

fn ensure_auction_bracket_slot_allowed(
    format: AuctionFormat,
    slot_key: &str,
) -> Result<(), AttemptError> {
    if allowed_auction_bracket_slot_keys(format).contains(&slot_key) {
        Ok(())
    } else {
        Err(AttemptError::AuctionTournamentBracketSlotUnavailable)
    }
}

fn ensure_auction_bracket_rosters(
    format: AuctionFormat,
    teams: &[AuctionTournamentTeamRow],
    members: &[AuctionTeamMemberLockRow],
) -> Result<(), AttemptError> {
    if teams.len() != usize::try_from(format.team_count()).unwrap_or_default() {
        return Err(AttemptError::AuctionTournamentBracketRosterIncomplete);
    }
    let team_ids = teams.iter().map(|team| team.id).collect::<HashSet<_>>();
    if team_ids.len() != teams.len()
        || members
            .iter()
            .any(|member| !team_ids.contains(&member.team_id))
        || teams.iter().any(|team| {
            members
                .iter()
                .filter(|member| member.team_id == team.id)
                .count()
                != 5
        })
    {
        return Err(AttemptError::AuctionTournamentBracketRosterIncomplete);
    }
    Ok(())
}

fn ensure_auction_bracket_lock_slots(
    format: AuctionFormat,
    slots: &[AuctionTournamentBracketSlotRow],
    teams: &[AuctionTournamentTeamRow],
) -> Result<(), AttemptError> {
    let allowed_slots = allowed_auction_bracket_slot_keys(format);
    let team_ids = teams.iter().map(|team| team.id).collect::<HashSet<_>>();
    let mut assigned = HashSet::new();
    for slot in slots {
        let allowed = allowed_slots.contains(&slot.slot_key.as_str());
        match (allowed, slot.team_id) {
            (true, Some(team_id)) if team_ids.contains(&team_id) && assigned.insert(team_id) => {}
            (true, _) => return Err(AttemptError::AuctionTournamentBracketSlotsIncomplete),
            (false, None) => {}
            (false, Some(_)) => return Err(AttemptError::AuctionTournamentBracketSlotUnavailable),
        }
    }
    if assigned == team_ids {
        Ok(())
    } else {
        Err(AttemptError::AuctionTournamentBracketSlotsIncomplete)
    }
}

fn tournament_bracket_rules(
    tournament: &AuctionTournamentBracketLockRow,
) -> Result<(AuctionFormat, AuctionRecruitmentSeriesRules), AttemptError> {
    let format = parse_auction_format(&tournament.format)?;
    let rules = AuctionRecruitmentSeriesRules {
        hard_fearless: tournament.hard_fearless,
        semifinal_best_of: tournament.semifinal_best_of,
        final_best_of: tournament.final_best_of,
    };
    validate_auction_recruitment_series_rules(format, &rules)
        .map_err(|_| AttemptError::InvariantViolation)?;
    Ok((format, rules))
}

fn bracket_slot_team_id(
    slots: &[AuctionTournamentBracketSlotRow],
    slot_key: &str,
) -> Result<Uuid, AttemptError> {
    auction_bracket_slot(slots, slot_key)?
        .team_id
        .ok_or(AttemptError::AuctionTournamentBracketSlotsIncomplete)
}

#[allow(clippy::too_many_arguments)]
async fn apply_auction_tournament_bracket_command(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    command: &AuctionTournamentBracketCommand,
    tournament: &AuctionTournamentBracketLockRow,
    teams: &[AuctionTournamentTeamRow],
    members: &[AuctionTeamMemberLockRow],
    slots: &[AuctionTournamentBracketSlotRow],
    prepared: WebAuctionTournamentBracketPrepared,
) -> Result<AuctionTournamentBracketCommandOutcome, AttemptError> {
    let (format, rules) = tournament_bracket_rules(tournament)?;
    match command {
        AuctionTournamentBracketCommand::AssignTeam {
            slot_key, team_id, ..
        } => {
            ensure_auction_bracket_slot_allowed(format, slot_key)?;
            let _target = auction_bracket_slot(slots, slot_key)?;
            let _team = auction_team(teams, *team_id)?;
            // Clear the old placement and target before assigning to satisfy the unique team index.
            sqlx::query(
                "UPDATE auction_bracket_slots
                 SET team_id = NULL, updated_by = $1, updated_at = $2
                 WHERE guild_id = $3
                   AND tournament_id = $4
                   AND (slot_key = $5 OR team_id = $6)",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(tournament_id.as_uuid())
            .bind(slot_key)
            .bind(team_id)
            .execute(&mut **transaction)
            .await?;
            let assigned = sqlx::query(
                "UPDATE auction_bracket_slots
                 SET team_id = $1, updated_by = $2, updated_at = $3
                 WHERE guild_id = $4 AND tournament_id = $5 AND slot_key = $6",
            )
            .bind(team_id)
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(tournament_id.as_uuid())
            .bind(slot_key)
            .execute(&mut **transaction)
            .await?;
            if assigned.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
            Ok(AuctionTournamentBracketCommandOutcome::Updated)
        }
        AuctionTournamentBracketCommand::ClearSlot { slot_key, .. } => {
            ensure_auction_bracket_slot_allowed(format, slot_key)?;
            let _target = auction_bracket_slot(slots, slot_key)?;
            let cleared = sqlx::query(
                "UPDATE auction_bracket_slots
                 SET team_id = NULL, updated_by = $1, updated_at = $2
                 WHERE guild_id = $3 AND tournament_id = $4 AND slot_key = $5",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(tournament_id.as_uuid())
            .bind(slot_key)
            .execute(&mut **transaction)
            .await?;
            if cleared.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
            Ok(AuctionTournamentBracketCommandOutcome::Updated)
        }
        AuctionTournamentBracketCommand::LockBracket { .. } => {
            ensure_auction_bracket_rosters(format, teams, members)?;
            ensure_auction_bracket_lock_slots(format, slots, teams)?;
            match format {
                AuctionFormat::TenPerson => {
                    insert_auction_tournament_match(
                        transaction,
                        guild_id,
                        tournament_id,
                        prepared.match_ids[2],
                        tournament.season_id,
                        "FINAL",
                        rules.final_best_of,
                        rules.hard_fearless,
                        Some(bracket_slot_team_id(slots, "A_0")?),
                        Some(bracket_slot_team_id(slots, "A_1")?),
                        actor_id,
                        prepared.now,
                    )
                    .await?;
                }
                AuctionFormat::TwentyPerson => {
                    let semifinal_best_of = rules
                        .semifinal_best_of
                        .ok_or(AttemptError::InvariantViolation)?;
                    insert_auction_tournament_match(
                        transaction,
                        guild_id,
                        tournament_id,
                        prepared.match_ids[0],
                        tournament.season_id,
                        "SEMIFINAL_A",
                        semifinal_best_of,
                        rules.hard_fearless,
                        Some(bracket_slot_team_id(slots, "A_0")?),
                        Some(bracket_slot_team_id(slots, "A_1")?),
                        actor_id,
                        prepared.now,
                    )
                    .await?;
                    insert_auction_tournament_match(
                        transaction,
                        guild_id,
                        tournament_id,
                        prepared.match_ids[1],
                        tournament.season_id,
                        "SEMIFINAL_B",
                        semifinal_best_of,
                        rules.hard_fearless,
                        Some(bracket_slot_team_id(slots, "B_0")?),
                        Some(bracket_slot_team_id(slots, "B_1")?),
                        actor_id,
                        prepared.now,
                    )
                    .await?;
                    insert_auction_tournament_match(
                        transaction,
                        guild_id,
                        tournament_id,
                        prepared.match_ids[2],
                        tournament.season_id,
                        "FINAL",
                        rules.final_best_of,
                        rules.hard_fearless,
                        None,
                        None,
                        actor_id,
                        prepared.now,
                    )
                    .await?;
                }
            }
            Ok(AuctionTournamentBracketCommandOutcome::Locked)
        }
        AuctionTournamentBracketCommand::UnlockBracket { .. } => {
            ensure_auction_tournament_matches_have_no_result_artifacts(
                transaction,
                guild_id,
                tournament_id,
            )
            .await?;
            cancel_unstarted_auction_tournament_matches(
                transaction,
                guild_id,
                tournament_id,
                actor_id,
                prepared.now,
            )
            .await?;
            Ok(AuctionTournamentBracketCommandOutcome::Unlocked)
        }
    }
}

async fn insert_auction_tournament_match(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    match_id: Uuid,
    season_id: Uuid,
    match_key: &str,
    best_of: i16,
    hard_fearless: bool,
    left_team_id: Option<Uuid>,
    right_team_id: Option<Uuid>,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "INSERT INTO auction_matches (
             guild_id, tournament_id, id, season_id, match_key, best_of, hard_fearless,
             left_team_id, right_team_id, status, winner_team_id, revision,
             lineups_locked_at, lineups_locked_by, started_at, ended_at, updated_by, updated_at,
             deleted_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7,
             $8, $9, 'LINEUP_PENDING', NULL, 0,
             NULL, NULL, NULL, NULL, $10, $11,
             NULL
         )",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(match_id)
    .bind(season_id)
    .bind(match_key)
    .bind(best_of)
    .bind(hard_fearless)
    .bind(left_team_id)
    .bind(right_team_id)
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn auction_join_lane(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    user_id: &UserId,
    declared_lane: Option<Lane>,
) -> Result<Lane, AttemptError> {
    if let Some(lane) = declared_lane {
        return Ok(lane);
    }

    sqlx::query_scalar::<_, Option<Lane>>(
        "SELECT main_lane
         FROM guild_members
         WHERE guild_id = $1 AND user_id = $2 AND status = 'ACTIVE'
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?
    .flatten()
    .ok_or(AttemptError::AuctionDeclaredLaneRequired)
}

async fn join_auction_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    user_id: &UserId,
    declared_lane: Lane,
    mmr_at_join: f64,
    target_count: i16,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let existing_status = sqlx::query_scalar::<_, String>(
        "SELECT status
         FROM auction_recruitment_participants
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if existing_status.as_deref() == Some("JOINED") {
        return Err(AttemptError::ParticipantAlreadyJoined);
    }

    let joined =
        joined_auction_recruitment_participant_count(transaction, guild_id, recruitment_id).await?;
    if joined >= i64::from(target_count) {
        return Err(AttemptError::RecruitmentFull);
    }

    sqlx::query(
        "INSERT INTO auction_recruitment_participants (
             guild_id, recruitment_id, user_id, status, declared_lane, mmr_at_join,
             snapshot_origin, joined_at, withdrawn_at
         ) VALUES ($1, $2, $3, 'JOINED', $4, $5, 'CAPTURED', $6, NULL)
         ON CONFLICT (guild_id, recruitment_id, user_id) DO UPDATE
         SET status = 'JOINED',
             declared_lane = EXCLUDED.declared_lane,
             mmr_at_join = EXCLUDED.mmr_at_join,
             snapshot_origin = 'CAPTURED',
             joined_at = EXCLUDED.joined_at,
             withdrawn_at = NULL",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .bind(declared_lane)
    .bind(mmr_at_join)
    .bind(now)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

async fn leave_auction_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    user_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let participant = sqlx::query(
        "UPDATE auction_recruitment_participants
         SET status = 'WITHDRAWN', withdrawn_at = $1
         WHERE guild_id = $2
           AND recruitment_id = $3
           AND user_id = $4
           AND status = 'JOINED'",
    )
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;
    if participant.rows_affected() != 1 {
        return Err(AttemptError::ParticipantNotJoined);
    }

    sqlx::query(
        "UPDATE auction_setup_teams
         SET captain_user_id = NULL
         WHERE guild_id = $1 AND recruitment_id = $2 AND captain_user_id = $3",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

async fn bump_auction_recruitment_and_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let recruitment = sqlx::query(
        "UPDATE auction_recruitments
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    let draft = sqlx::query(
        "UPDATE auction_setup_drafts
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND recruitment_id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if recruitment.rows_affected() != 1 || draft.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }
    Ok(())
}

async fn join_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    participant_id: &UserId,
    target_count: i16,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let existing_status = sqlx::query_scalar::<_, String>(
        "SELECT status
         FROM recruitment_participants
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(participant_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if existing_status.as_deref() == Some("JOINED") {
        return Err(AttemptError::ParticipantAlreadyJoined);
    }

    let joined_count = joined_participant_count(transaction, guild_id, recruitment_id).await?;
    if joined_count >= i64::from(target_count) {
        return Err(AttemptError::RecruitmentFull);
    }

    sqlx::query(
        "INSERT INTO recruitment_participants (
             guild_id, recruitment_id, user_id, status, joined_at, withdrawn_at
         ) VALUES ($1, $2, $3, 'JOINED', $4, NULL)
         ON CONFLICT (guild_id, recruitment_id, user_id) DO UPDATE
         SET status = 'JOINED', joined_at = EXCLUDED.joined_at, withdrawn_at = NULL",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(participant_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

async fn leave_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    user_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let updated = sqlx::query(
        "UPDATE recruitment_participants
         SET status = 'WITHDRAWN', withdrawn_at = $1
         WHERE guild_id = $2
           AND recruitment_id = $3
           AND user_id = $4
           AND status = 'JOINED'",
    )
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AttemptError::ParticipantNotJoined);
    }

    sqlx::query(
        "DELETE FROM recruitment_entry_assignments
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

async fn set_preferred_lanes(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    user_id: &UserId,
    lanes: &[Lane],
) -> Result<(), AttemptError> {
    let participant_status = sqlx::query_scalar::<_, String>(
        "SELECT status
         FROM recruitment_participants
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if participant_status.as_deref() != Some("JOINED") {
        return Err(AttemptError::ParticipantNotJoined);
    }

    sqlx::query(
        "DELETE FROM recruitment_preferred_lanes
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;

    for (index, lane) in lanes.iter().enumerate() {
        let priority = i16::try_from(index + 1).map_err(|_| AttemptError::InvalidPreferredLanes)?;
        sqlx::query(
            "INSERT INTO recruitment_preferred_lanes (
                 guild_id, recruitment_id, user_id, priority, lane
             ) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(guild_id)
        .bind(recruitment_id.as_uuid())
        .bind(user_id)
        .bind(priority)
        .bind(*lane)
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}

async fn entry_draft_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    viewer_id: &UserId,
) -> Result<EntryDraftSnapshot, AttemptError> {
    let recruitment = sqlx::query_as::<_, EntryDraftRecruitmentRow>(
        "SELECT recruitment.id,
                recruitment.season_id,
                recruitment.display_number,
                recruitment.title,
                recruitment.status,
                recruitment.target_count,
                recruitment.best_of,
                recruitment.hard_fearless,
                recruitment.revision,
                recruitment.updated_at,
                recruitment.converted_at,
                recruitment.created_by,
                draft.revision AS draft_revision,
                draft.updated_at AS draft_updated_at,
                draft.updated_by AS draft_updated_by,
                editor.display_name AS draft_updated_by_display_name
         FROM recruitments AS recruitment
         JOIN recruitment_entry_drafts AS draft
           ON draft.guild_id = recruitment.guild_id
          AND draft.recruitment_id = recruitment.id
         LEFT JOIN guild_members AS editor
           ON editor.guild_id = draft.guild_id
          AND editor.user_id = draft.updated_by
         WHERE recruitment.guild_id = $1 AND recruitment.id = $2",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::RecruitmentNotFound)?;

    let participant_rows = sqlx::query_as::<_, EntryDraftParticipantRow>(
        "SELECT participant.user_id AS id,
                member.display_name,
                member.main_lane,
                COALESCE(mmr.mmr, 1500::double precision) AS mmr,
                COALESCE(mmr.games_played, 0) AS games,
                COALESCE(mmr.wins, 0) AS wins,
                solo_rank.tier AS solo_rank_tier,
                solo_rank.division AS solo_rank_division,
                account.profile_icon_id
         FROM recruitment_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         JOIN recruitments AS recruitment
           ON recruitment.guild_id = participant.guild_id
          AND recruitment.id = participant.recruitment_id
         LEFT JOIN recruitment_entry_assignments AS assignment
           ON assignment.guild_id = participant.guild_id
          AND assignment.recruitment_id = participant.recruitment_id
          AND assignment.user_id = participant.user_id
         LEFT JOIN LATERAL (
             SELECT state.mmr, state.games_played, state.wins
             FROM user_lane_mmr AS state
             WHERE state.guild_id = participant.guild_id
               AND state.season_id = recruitment.season_id
               AND state.user_id = participant.user_id
               AND state.lane = COALESCE(
                   assignment.lane,
                   member.main_lane,
                   (
                       SELECT preferred.lane
                       FROM recruitment_preferred_lanes AS preferred
                       WHERE preferred.guild_id = participant.guild_id
                         AND preferred.recruitment_id = participant.recruitment_id
                         AND preferred.user_id = participant.user_id
                       ORDER BY preferred.priority
                       LIMIT 1
                   )
               )
             LIMIT 1
         ) AS mmr ON TRUE
         LEFT JOIN LATERAL (
             SELECT riot.platform, riot.puuid, riot.profile_icon_id
             FROM riot_accounts AS riot
             WHERE riot.user_id = participant.user_id
               AND riot.is_main = true
               AND riot.origin = 'RIOT'
             ORDER BY riot.updated_at DESC
             LIMIT 1
         ) AS account ON TRUE
         LEFT JOIN LATERAL (
             SELECT rank_entry.value ->> 'tier' AS tier,
                    rank_entry.value ->> 'rank' AS division
             FROM riot_records_snapshots AS records
             CROSS JOIN LATERAL jsonb_array_elements(
                 CASE
                     WHEN jsonb_typeof(records.snapshot -> 'ranks') = 'array'
                         THEN records.snapshot -> 'ranks'
                     ELSE '[]'::jsonb
                 END
             ) AS rank_entry(value)
             WHERE records.platform = account.platform
               AND records.puuid = account.puuid
               AND rank_entry.value ->> 'queueType' = 'RANKED_SOLO_5x5'
             LIMIT 1
         ) AS solo_rank ON TRUE
         WHERE participant.guild_id = $1
           AND participant.recruitment_id = $2
           AND participant.status = 'JOINED'
         ORDER BY participant.joined_at, participant.user_id",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let preferred_lane_rows = sqlx::query_as::<_, RecruitmentLaneRow>(
        "SELECT preferred.user_id, preferred.lane
         FROM recruitment_preferred_lanes AS preferred
         JOIN recruitment_participants AS participant
           ON participant.guild_id = preferred.guild_id
          AND participant.recruitment_id = preferred.recruitment_id
          AND participant.user_id = preferred.user_id
          AND participant.status = 'JOINED'
         WHERE preferred.guild_id = $1 AND preferred.recruitment_id = $2
         ORDER BY preferred.user_id, preferred.priority",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let champion_rows = sqlx::query_as::<_, EntryDraftChampionRow>(
        "SELECT preference.user_id, preference.champion_id
         FROM user_champion_preferences AS preference
         JOIN recruitment_participants AS participant
           ON participant.user_id = preference.user_id
          AND participant.guild_id = $1
          AND participant.recruitment_id = $2
          AND participant.status = 'JOINED'
         WHERE preference.user_id IN (
             SELECT joined.user_id
             FROM recruitment_participants AS joined
             WHERE joined.guild_id = $1
               AND joined.recruitment_id = $2
               AND joined.status = 'JOINED'
         )
         ORDER BY preference.user_id,
                  CASE preference.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END,
                  preference.position",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let assignment_rows = sqlx::query_as::<_, EntryDraftAssignmentRow>(
        "SELECT assignment.user_id AS participant_id, assignment.team, assignment.lane
         FROM recruitment_entry_assignments AS assignment
         JOIN recruitment_participants AS participant
           ON participant.guild_id = assignment.guild_id
          AND participant.recruitment_id = assignment.recruitment_id
          AND participant.user_id = assignment.user_id
          AND participant.status = 'JOINED'
         WHERE assignment.guild_id = $1 AND assignment.recruitment_id = $2
         ORDER BY assignment.team,
                  CASE assignment.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;

    let mut lanes_by_user: HashMap<UserId, Vec<Lane>> = HashMap::new();
    for row in preferred_lane_rows {
        lanes_by_user.entry(row.user_id).or_default().push(row.lane);
    }
    let mut champions_by_user: HashMap<UserId, Vec<String>> = HashMap::new();
    for row in champion_rows {
        champions_by_user
            .entry(row.user_id)
            .or_default()
            .push(row.champion_id);
    }
    let participants = participant_rows
        .into_iter()
        .map(|row| EntryDraftParticipantSnapshot {
            preferred_lanes: lanes_by_user.remove(&row.id).unwrap_or_default(),
            champion_ids: champions_by_user.remove(&row.id).unwrap_or_default(),
            id: row.id,
            display_name: row.display_name,
            main_lane: row.main_lane,
            mmr: row.mmr,
            games: row.games,
            wins: row.wins,
            riot_tier: compact_riot_tier(
                row.solo_rank_tier.as_deref(),
                row.solo_rank_division.as_deref(),
            ),
            profile_icon_id: row.profile_icon_id,
        })
        .collect::<Vec<_>>();
    let head_to_head = entry_draft_head_to_head_snapshot(
        transaction,
        guild_id,
        recruitment_id,
        recruitment.season_id,
    )
    .await?;
    let is_operator = is_active_session_operator(transaction, guild_id, viewer_id).await?;
    let can_edit = recruitment.status == RecruitmentStatus::Closed && is_operator;
    let has_complete_assignments = has_complete_entry_assignments(
        i64::try_from(participants.len()).map_err(|_| AttemptError::InvariantViolation)?,
        &assignment_rows,
        recruitment.target_count,
    );
    let participant_count =
        i64::try_from(participants.len()).map_err(|_| AttemptError::InvariantViolation)?;
    let has_full_roster = recruitment.target_count == RECRUITMENT_TARGET_COUNT
        && participant_count == i64::from(recruitment.target_count);
    let can_create_series =
        recruitment.status == RecruitmentStatus::Closed && is_operator && has_complete_assignments;
    let active_series_exists = if recruitment.status == RecruitmentStatus::Closed
        && is_operator
        && (has_complete_assignments || has_full_roster)
    {
        active_series_exists_for_source_recruitment(transaction, guild_id, recruitment_id).await?
    } else {
        false
    };
    let can_create_auction_recruitment = recruitment.status == RecruitmentStatus::Closed
        && is_operator
        && has_full_roster
        && recruitment.converted_at.is_none()
        && !active_series_exists;
    let can_cancel = matches!(
        recruitment.status,
        RecruitmentStatus::Open | RecruitmentStatus::Closed
    ) && (is_operator || recruitment.created_by == *viewer_id);

    Ok(EntryDraftSnapshot {
        recruitment: EntryDraftRecruitmentSnapshot {
            id: RecruitmentId::from_uuid(recruitment.id),
            display_number: recruitment.display_number,
            title: recruitment.title,
            status: recruitment.status,
            target_count: recruitment.target_count,
            best_of: recruitment.best_of,
            hard_fearless: recruitment.hard_fearless,
            revision: recruitment.revision,
            updated_at: recruitment.updated_at,
        },
        draft: EntryDraftStateSnapshot {
            revision: recruitment.draft_revision,
            updated_at: recruitment.draft_updated_at,
            updated_by: match (
                recruitment.draft_updated_by,
                recruitment.draft_updated_by_display_name,
            ) {
                (Some(id), Some(display_name)) => {
                    Some(EntryDraftEditorSnapshot { id, display_name })
                }
                (None, None) => None,
                _ => return Err(AttemptError::InvariantViolation),
            },
            participants,
            assignments: assignment_rows
                .into_iter()
                .map(EntryDraftAssignmentRow::into_snapshot)
                .collect::<Result<_, _>>()?,
        },
        head_to_head,
        capabilities: EntryDraftCapabilities {
            can_edit,
            can_reset: can_edit,
            can_create_series,
            can_create_auction_recruitment,
            can_send_participant_confirmation_dms: recruitment.status == RecruitmentStatus::Closed
                && is_operator,
            can_reopen: recruitment.status == RecruitmentStatus::Closed && is_operator,
            can_cancel,
        },
    })
}

async fn entry_draft_head_to_head_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    season_id: Uuid,
) -> Result<Vec<EntryDraftHeadToHeadSnapshot>, AttemptError> {
    let rows = sqlx::query_as::<_, EntryDraftHeadToHeadRow>(
        "SELECT ledger.lane,
                ledger.user_id AS user_a_id,
                ledger.opponent_user_id AS user_b_id,
                COUNT(*) AS plays,
                SUM(ledger.wins_delta)::bigint AS user_a_wins,
                (COUNT(*) - SUM(ledger.wins_delta))::bigint AS user_b_wins
         FROM mmr_ledger_entries AS ledger
         JOIN games AS game
           ON game.guild_id = ledger.guild_id
          AND game.id = ledger.game_id
          AND game.status = 'CONFIRMED'
          AND game.series_id IS NOT NULL
         JOIN recruitment_participants AS user_a
           ON user_a.guild_id = ledger.guild_id
          AND user_a.recruitment_id = $2
          AND user_a.user_id = ledger.user_id
          AND user_a.status = 'JOINED'
         JOIN recruitment_participants AS user_b
           ON user_b.guild_id = ledger.guild_id
          AND user_b.recruitment_id = $2
          AND user_b.user_id = ledger.opponent_user_id
          AND user_b.status = 'JOINED'
         WHERE ledger.guild_id = $1
           AND ledger.season_id = $3
           AND ledger.source_type = 'GAME_RESULT'
           AND ledger.user_id < ledger.opponent_user_id
         GROUP BY ledger.lane, ledger.user_id, ledger.opponent_user_id
         ORDER BY CASE ledger.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END,
                  ledger.user_id,
                  ledger.opponent_user_id",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(season_id)
    .fetch_all(&mut **transaction)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| EntryDraftHeadToHeadSnapshot {
            lane: row.lane,
            user_a_id: row.user_a_id,
            user_b_id: row.user_b_id,
            plays: row.plays,
            user_a_wins: row.user_a_wins,
            user_b_wins: row.user_b_wins,
        })
        .collect())
}

async fn series_pick_ban_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
    viewer_id: &UserId,
) -> Result<SeriesPickBanSnapshot, AttemptError> {
    let series = sqlx::query_as::<_, SeriesPickBanSeriesRow>(
        "SELECT id, display_number, title, best_of, hard_fearless, status, revision,
                source_recruitment_id, updated_at
         FROM series
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::SeriesNotFound)?;
    ensure_series_in_progress(series.status)?;

    let (teams, player_ids) = series_pick_ban_teams(transaction, guild_id, series_id).await?;
    let confirmed_game_count =
        confirmed_series_game_count(transaction, guild_id, series_id).await?;
    let next_set_number =
        i16::try_from(confirmed_game_count + 1).map_err(|_| AttemptError::InvariantViolation)?;
    if next_set_number > series.best_of {
        return Err(AttemptError::InvariantViolation);
    }
    let draft = sqlx::query_as::<_, SeriesPickBanGameInputRow>(
        "SELECT id, set_number, side_preset, side_selected, winning_side, patch_version
         FROM game_inputs
         WHERE guild_id = $1
           AND series_id = $2
           AND set_number = $3
           AND status = 'DRAFT'",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .bind(next_set_number)
    .fetch_optional(&mut **transaction)
    .await?;
    let current_game = match draft {
        Some(draft) => {
            series_pick_ban_game_snapshot(transaction, guild_id, &draft, &player_ids).await?
        }
        None => empty_series_pick_ban_game(next_set_number, &player_ids),
    };
    let games =
        confirmed_series_game_snapshots(transaction, guild_id, series_id, &player_ids).await?;
    let can_edit = is_active_session_operator(transaction, guild_id, viewer_id).await?;

    Ok(SeriesPickBanSnapshot {
        series: SeriesPickBanSeriesSnapshot {
            id: SeriesId::from_uuid(series.id),
            display_number: series.display_number,
            title: series.title,
            best_of: series.best_of,
            hard_fearless: series.hard_fearless,
            status: series.status,
            revision: series.revision,
            source_recruitment_id: RecruitmentId::from_uuid(series.source_recruitment_id),
            updated_at: series.updated_at,
        },
        teams,
        current_game,
        games,
        capabilities: SeriesPickBanCapabilities {
            can_edit,
            can_submit: can_edit,
            can_revert_last_game: can_edit && confirmed_game_count > 0,
            can_early_complete: can_edit
                && can_early_complete_series(series.best_of, confirmed_game_count),
            can_revert: can_edit,
            can_delete: can_edit,
        },
    })
}

async fn series_result_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
    viewer_id: &UserId,
) -> Result<SeriesResultSnapshot, AttemptError> {
    let series = sqlx::query_as::<_, SeriesResultSeriesRow>(
        "SELECT id, display_number, title, best_of, hard_fearless, status, revision, source_recruitment_id,
                winning_team, terminal_reason, started_at, ended_at
         FROM series
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::SeriesNotFound)?;
    if series.status != SeriesStatus::Completed {
        return Err(AttemptError::SeriesResultUnavailable(series.status));
    }

    let (teams, player_ids) = series_pick_ban_teams(transaction, guild_id, series_id).await?;
    let game_rows = sqlx::query_as::<_, SeriesResultGameRow>(
        "SELECT game.id,
                game.source_input_id,
                game.game_number AS set_number,
                game.side_preset,
                game.winning_side,
                game.patch_version,
                game.duration_seconds
         FROM games AS game
         WHERE game.guild_id = $1
           AND game.series_id = $2
           AND game.status = 'CONFIRMED'
         ORDER BY game.game_number",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let mut games = Vec::with_capacity(game_rows.len());
    for row in game_rows {
        let input = SeriesPickBanGameInputRow {
            id: row.source_input_id,
            set_number: row.set_number,
            side_preset: row.side_preset,
            side_selected: true,
            winning_side: Some(row.winning_side.clone()),
            patch_version: row.patch_version.clone(),
        };
        let draft =
            series_pick_ban_game_snapshot(transaction, guild_id, &input, &player_ids).await?;
        let winner = draft.winner.ok_or(AttemptError::InvariantViolation)?;
        let mmr_changes = sqlx::query_as::<_, SeriesResultMmrChangeRow>(
            "SELECT user_id, lane, delta, mmr_before, mmr_after
             FROM mmr_ledger_entries
             WHERE guild_id = $1
               AND game_id = $2
               AND source_type = 'GAME_RESULT'
             ORDER BY CASE lane
                          WHEN 'TOP' THEN 1
                          WHEN 'JUNGLE' THEN 2
                          WHEN 'MID' THEN 3
                          WHEN 'BOTTOM' THEN 4
                          WHEN 'SUPPORT' THEN 5
                      END,
                      user_id",
        )
        .bind(guild_id)
        .bind(row.id)
        .fetch_all(&mut **transaction)
        .await?
        .into_iter()
        .map(|change| SeriesResultMmrChange {
            user_id: change.user_id,
            lane: change.lane,
            delta: change.delta,
            mmr_before: change.mmr_before,
            mmr_after: change.mmr_after,
        })
        .collect();
        games.push(SeriesResultGameSnapshot {
            id: GameId::from_uuid(row.id),
            set_number: row.set_number,
            side_preset: draft.side_preset,
            winner,
            picks: draft.picks,
            bans: draft.bans,
            patch_version: row.patch_version,
            duration_seconds: row.duration_seconds,
            mmr_changes,
        });
    }
    let (team_1, team_2) = confirmed_series_score(transaction, guild_id, series_id).await?;
    let is_operator = is_active_session_operator(transaction, guild_id, viewer_id).await?;

    Ok(SeriesResultSnapshot {
        series: SeriesResultSeriesSnapshot {
            id: SeriesId::from_uuid(series.id),
            display_number: series.display_number,
            title: series.title,
            best_of: series.best_of,
            hard_fearless: series.hard_fearless,
            status: series.status,
            revision: series.revision,
            source_recruitment_id: RecruitmentId::from_uuid(series.source_recruitment_id),
            winning_team: series.winning_team,
            terminal_reason: series.terminal_reason,
            started_at: series.started_at,
            ended_at: series.ended_at,
        },
        teams,
        games,
        score: SeriesResultScoreSnapshot { team_1, team_2 },
        capabilities: SeriesResultCapabilities {
            can_revert_last_game: is_operator,
            can_delete: is_operator,
        },
    })
}

async fn series_pick_ban_teams(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<(Vec<SeriesPickBanTeamSnapshot>, BTreeSet<String>), AttemptError> {
    let participant_rows = sqlx::query_as::<_, SeriesPickBanParticipantRow>(
        "SELECT participant.team,
                participant.user_id AS id,
                participant.lane,
                participant.mmr_at_lock AS mmr,
                member.display_name,
                solo_rank.tier AS solo_rank_tier,
                solo_rank.division AS solo_rank_division
         FROM series_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         LEFT JOIN LATERAL (
             SELECT riot.platform, riot.puuid
             FROM riot_accounts AS riot
             WHERE riot.user_id = participant.user_id
               AND riot.is_main = true
               AND riot.origin = 'RIOT'
             ORDER BY riot.updated_at DESC
             LIMIT 1
         ) AS account ON TRUE
         LEFT JOIN LATERAL (
             SELECT rank_entry.value ->> 'tier' AS tier,
                    rank_entry.value ->> 'rank' AS division
             FROM riot_records_snapshots AS records
             CROSS JOIN LATERAL jsonb_array_elements(
                 CASE
                     WHEN jsonb_typeof(records.snapshot -> 'ranks') = 'array'
                         THEN records.snapshot -> 'ranks'
                     ELSE '[]'::jsonb
                 END
             ) AS rank_entry(value)
             WHERE records.platform = account.platform
               AND records.puuid = account.puuid
               AND rank_entry.value ->> 'queueType' = 'RANKED_SOLO_5x5'
             LIMIT 1
         ) AS solo_rank ON TRUE
         WHERE participant.guild_id = $1 AND participant.series_id = $2
         ORDER BY CASE participant.team
                      WHEN 'TEAM_1' THEN 1
                      WHEN 'TEAM_2' THEN 2
                  END,
                  CASE participant.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let champion_rows = sqlx::query_as::<_, SeriesPickBanChampionRow>(
        "SELECT preference.user_id, preference.champion_id
         FROM user_champion_preferences AS preference
         JOIN series_participants AS participant
           ON participant.user_id = preference.user_id
          AND participant.guild_id = $1
          AND participant.series_id = $2
          AND participant.lane = preference.lane
         ORDER BY preference.user_id,
                  preference.position",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;

    let mut champions_by_user: HashMap<UserId, Vec<String>> = HashMap::new();
    for row in champion_rows {
        champions_by_user
            .entry(row.user_id)
            .or_default()
            .push(row.champion_id);
    }
    let mut team_1 = Vec::with_capacity(5);
    let mut team_2 = Vec::with_capacity(5);
    for row in participant_rows {
        let player = SeriesPickBanPlayerSnapshot {
            preferred_champion_ids: champions_by_user.remove(&row.id).unwrap_or_default(),
            id: row.id,
            display_name: row.display_name,
            lane: row.lane,
            mmr: row.mmr.ok_or(AttemptError::InvariantViolation)?,
            riot_tier: compact_riot_tier(
                row.solo_rank_tier.as_deref(),
                row.solo_rank_division.as_deref(),
            ),
        };
        match row.team.as_str() {
            "TEAM_1" => team_1.push(player),
            "TEAM_2" => team_2.push(player),
            _ => return Err(AttemptError::InvariantViolation),
        }
    }
    ensure_complete_series_roster(&team_1, &team_2)?;
    let teams = vec![
        SeriesPickBanTeamSnapshot {
            team: "TEAM_1".to_owned(),
            name: "1팀".to_owned(),
            players: team_1,
        },
        SeriesPickBanTeamSnapshot {
            team: "TEAM_2".to_owned(),
            name: "2팀".to_owned(),
            players: team_2,
        },
    ];
    let player_ids = teams
        .iter()
        .flat_map(|team| {
            team.players
                .iter()
                .map(|player| player.id.as_str().to_owned())
        })
        .collect::<BTreeSet<_>>();
    Ok((teams, player_ids))
}

async fn series_pick_ban_game_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    input: &SeriesPickBanGameInputRow,
    player_ids: &BTreeSet<String>,
) -> Result<SeriesPickBanGameSnapshot, AttemptError> {
    let mut snapshot = empty_series_pick_ban_game(input.set_number, player_ids);
    snapshot.side_preset = canonical_side_preset(&input.side_preset)?;
    snapshot.side_selected = input.side_selected || input.winning_side.is_some();
    snapshot.winner = input
        .winning_side
        .as_deref()
        .map(canonical_blue_red_side)
        .transpose()?;
    snapshot.patch_version = input.patch_version.clone();

    let picks = sqlx::query_as::<_, SeriesPickBanPickRow>(
        "SELECT user_id, champion_id
         FROM game_input_picks
         WHERE guild_id = $1 AND game_input_id = $2",
    )
    .bind(guild_id)
    .bind(input.id)
    .fetch_all(&mut **transaction)
    .await?;
    for pick in picks {
        if !player_ids.contains(pick.user_id.as_str()) {
            return Err(AttemptError::InvariantViolation);
        }
        snapshot
            .picks
            .insert(pick.user_id.to_string(), Some(pick.champion_id));
    }
    let bans = sqlx::query_as::<_, SeriesPickBanBanRow>(
        "SELECT side, slot, champion_id
         FROM game_input_bans
         WHERE guild_id = $1 AND game_input_id = $2",
    )
    .bind(guild_id)
    .bind(input.id)
    .fetch_all(&mut **transaction)
    .await?;
    for ban in bans {
        let index = usize::try_from(
            ban.slot
                .checked_sub(1)
                .ok_or(AttemptError::InvariantViolation)?,
        )
        .map_err(|_| AttemptError::InvariantViolation)?;
        let slots = match ban.side.as_str() {
            "BLUE" => &mut snapshot.bans.blue,
            "RED" => &mut snapshot.bans.red,
            _ => return Err(AttemptError::InvariantViolation),
        };
        let slot = slots
            .get_mut(index)
            .ok_or(AttemptError::InvariantViolation)?;
        if slot.replace(ban.champion_id).is_some() {
            return Err(AttemptError::InvariantViolation);
        }
    }
    Ok(snapshot)
}

async fn confirmed_series_game_snapshots(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
    player_ids: &BTreeSet<String>,
) -> Result<Vec<SeriesPickBanGameSnapshot>, AttemptError> {
    let inputs = sqlx::query_as::<_, SeriesPickBanGameInputRow>(
        "SELECT input.id, input.set_number, input.side_preset, input.side_selected,
                input.winning_side, input.patch_version
         FROM games AS game
         JOIN game_inputs AS input
           ON input.guild_id = game.guild_id AND input.id = game.source_input_id
         WHERE game.guild_id = $1 AND game.series_id = $2 AND game.status = 'CONFIRMED'
         ORDER BY game.game_number",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let mut games = Vec::with_capacity(inputs.len());
    for input in inputs {
        let game = series_pick_ban_game_snapshot(transaction, guild_id, &input, player_ids).await?;
        if game.winner.is_none() {
            return Err(AttemptError::InvariantViolation);
        }
        games.push(game);
    }
    Ok(games)
}

fn empty_series_pick_ban_game(
    set_number: i16,
    player_ids: &BTreeSet<String>,
) -> SeriesPickBanGameSnapshot {
    SeriesPickBanGameSnapshot {
        set_number,
        side_preset: "DEFAULT".to_owned(),
        side_selected: false,
        winner: None,
        picks: player_ids
            .iter()
            .map(|id| (id.clone(), None))
            .collect::<BTreeMap<_, _>>(),
        bans: SeriesPickBanBansSnapshot {
            blue: vec![None; 5],
            red: vec![None; 5],
        },
        patch_version: None,
    }
}

fn ensure_complete_series_roster(
    team_1: &[SeriesPickBanPlayerSnapshot],
    team_2: &[SeriesPickBanPlayerSnapshot],
) -> Result<(), AttemptError> {
    for team in [team_1, team_2] {
        if team.len() != 5 {
            return Err(AttemptError::InvariantViolation);
        }
        let lanes = team
            .iter()
            .map(|player| player.lane)
            .collect::<HashSet<_>>();
        if lanes.len() != 5 {
            return Err(AttemptError::InvariantViolation);
        }
    }
    Ok(())
}

fn canonical_side_preset(value: &str) -> Result<String, AttemptError> {
    match value {
        "DEFAULT" | "SWAPPED" => Ok(value.to_owned()),
        _ => Err(AttemptError::InvariantViolation),
    }
}

fn canonical_blue_red_side(value: &str) -> Result<String, AttemptError> {
    match value {
        "BLUE" | "RED" => Ok(value.to_owned()),
        _ => Err(AttemptError::InvariantViolation),
    }
}

async fn recruitment_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let recruitment = sqlx::query_as::<_, RecruitmentProjectionRow>(
        "SELECT id, display_number, title, status, target_count, best_of, hard_fearless, revision,
                channel_id, message_id, created_by, updated_at
         FROM recruitments
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::RecruitmentNotFound)?
    .into_snapshot();

    let participant_rows = sqlx::query_as::<_, RecruitmentParticipantRow>(
        "SELECT participant.user_id AS id, member.display_name
         FROM recruitment_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         WHERE participant.guild_id = $1
           AND participant.recruitment_id = $2
           AND participant.status = 'JOINED'
         ORDER BY participant.joined_at, participant.user_id",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let lane_rows = sqlx::query_as::<_, RecruitmentLaneRow>(
        "SELECT preferred.user_id, preferred.lane
         FROM recruitment_preferred_lanes AS preferred
         JOIN recruitment_participants AS participant
           ON participant.guild_id = preferred.guild_id
          AND participant.recruitment_id = preferred.recruitment_id
          AND participant.user_id = preferred.user_id
          AND participant.status = 'JOINED'
         WHERE preferred.guild_id = $1 AND preferred.recruitment_id = $2
         ORDER BY preferred.user_id, preferred.priority",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let mut lanes_by_user: HashMap<UserId, Vec<Lane>> = HashMap::new();
    for row in lane_rows {
        lanes_by_user.entry(row.user_id).or_default().push(row.lane);
    }
    let participants = participant_rows
        .into_iter()
        .map(|row| RecruitmentParticipantSnapshot {
            preferred_lanes: lanes_by_user.remove(&row.id).unwrap_or_default(),
            id: row.id,
            display_name: row.display_name,
        })
        .collect();

    Ok(RecruitmentSnapshot {
        recruitment,
        participants,
        links: RecruitmentLinksSnapshot {
            entry: format!("/g/{guild_id}/recruitments/{recruitment_id}/entry"),
        },
    })
}

fn registered_riot_account(account: RiotAccountRecord) -> RegisteredRiotAccountSnapshot {
    RegisteredRiotAccountSnapshot {
        puuid: account.puuid,
        game_name: account.game_name,
        tag_line: account.tag_line,
        platform: account.platform,
        is_main: account.is_main,
    }
}

async fn upsert_user(
    transaction: &mut DatabaseTransaction,
    actor: &BotActor,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let user_id = UserId::from(&actor.id);
    let persisted_user_id = sqlx::query_scalar::<_, String>(
        "INSERT INTO users (
             user_id, identity_kind, display_name, avatar_hash,
             created_at, updated_at, deleted_at
         ) VALUES ($1, 'DISCORD', $2, $3, $4, $4, NULL)
         ON CONFLICT (user_id) DO UPDATE
         SET display_name = EXCLUDED.display_name,
             avatar_hash = EXCLUDED.avatar_hash,
             updated_at = EXCLUDED.updated_at,
             deleted_at = NULL
         WHERE users.identity_kind = 'DISCORD'
         RETURNING user_id",
    )
    .bind(&user_id)
    .bind(actor.global_display_name.trim())
    .bind(actor.avatar_hash.as_deref())
    .bind(now)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::IdentityConflict)?;

    let identity_user_id = sqlx::query_scalar::<_, String>(
        "INSERT INTO user_discord_identities (
             user_id, discord_id, created_at, updated_at
         ) VALUES ($1, $2, $3, $3)
         ON CONFLICT (discord_id) DO UPDATE
         SET updated_at = EXCLUDED.updated_at
         WHERE user_discord_identities.user_id = EXCLUDED.user_id
         RETURNING user_id",
    )
    .bind(&user_id)
    .bind(&actor.id)
    .bind(now)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::IdentityConflict)?;
    if identity_user_id != persisted_user_id {
        return Err(AttemptError::IdentityConflict);
    }

    Ok(())
}

async fn sync_guild_actor(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor: &BotActor,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    upsert_user(transaction, actor, now).await?;
    sqlx::query(
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane, first_seen_at,
             registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
         ) VALUES ($1, $2, $3, 'ACTIVE', NULL, $4, $4, NULL, $4, $4, NULL)
         ON CONFLICT (guild_id, user_id) DO UPDATE
         SET display_name = EXCLUDED.display_name,
             registered_at = COALESCE(guild_members.registered_at, EXCLUDED.registered_at),
             last_active_at = EXCLUDED.last_active_at,
             last_synced_at = EXCLUDED.last_synced_at",
    )
    .bind(guild_id)
    .bind(&actor.id)
    .bind(actor.display_name.trim())
    .bind(now)
    .execute(&mut **transaction)
    .await?;

    sqlx::query("DELETE FROM guild_member_discord_roles WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(&actor.id)
        .execute(&mut **transaction)
        .await?;

    let mut inserted_roles = HashSet::new();
    for role_id in &actor.role_ids {
        if !inserted_roles.insert(role_id.as_str()) {
            continue;
        }
        sqlx::query(
            "INSERT INTO guild_member_discord_roles (
                 guild_id, user_id, role_id, last_synced_at
             ) VALUES ($1, $2, $3, $4)",
        )
        .bind(guild_id)
        .bind(&actor.id)
        .bind(role_id)
        .bind(now)
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}

async fn require_active_member(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    user_id: &DiscordUserId,
) -> Result<(), AttemptError> {
    let status = sqlx::query_scalar::<_, GuildMemberStatus>(
        "SELECT status
         FROM guild_members
         WHERE guild_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MemberInactive)?;
    if status == GuildMemberStatus::Active {
        Ok(())
    } else {
        Err(AttemptError::MemberInactive)
    }
}

async fn require_active_member_for_creation(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    user_id: &UserId,
) -> Result<(), AttemptError> {
    let status = sqlx::query_scalar::<_, GuildMemberStatus>(
        "SELECT status
         FROM guild_members
         WHERE guild_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if status == Some(GuildMemberStatus::Active) {
        Ok(())
    } else {
        Err(AttemptError::MembershipRequired)
    }
}

async fn require_active_recruitment_member(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    user_id: &UserId,
) -> Result<(), AttemptError> {
    match sqlx::query_scalar::<_, GuildMemberStatus>(
        "SELECT status
         FROM guild_members
         WHERE guild_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?
    {
        Some(GuildMemberStatus::Active) => Ok(()),
        Some(GuildMemberStatus::Inactive) => Err(AttemptError::RecruitmentMemberInactive),
        None => Err(AttemptError::RecruitmentMemberNotFound),
    }
}

async fn require_recruitment_channel(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<DiscordChannelId, AttemptError> {
    sqlx::query_scalar::<_, Option<DiscordChannelId>>(
        "SELECT recruitment_channel_id
         FROM guild_config
         WHERE guild_id = $1
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_one(&mut **transaction)
    .await?
    .ok_or(AttemptError::RecruitmentChannelNotConfigured)
}

async fn configured_entry_channel(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<Option<DiscordChannelId>, AttemptError> {
    sqlx::query_scalar::<_, Option<DiscordChannelId>>(
        "SELECT entry_channel_id
         FROM guild_config
         WHERE guild_id = $1",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::GuildNotConfigured)
}

async fn configured_series_result_channel(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<Option<DiscordChannelId>, AttemptError> {
    sqlx::query_scalar::<_, Option<DiscordChannelId>>(
        "SELECT result_channel_id
         FROM guild_config
         WHERE guild_id = $1",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::GuildNotConfigured)
}

async fn entry_confirmation_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
    display_number: i64,
    title: &str,
    best_of: i16,
    hard_fearless: bool,
) -> Result<EntryConfirmationSnapshot, AttemptError> {
    let rows = sqlx::query_as::<_, EntryConfirmationParticipantRow>(
        "SELECT participant.team,
                participant.user_id AS id,
                participant.lane,
                member.display_name
         FROM series_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         WHERE participant.guild_id = $1 AND participant.series_id = $2
         ORDER BY CASE participant.team
                      WHEN 'TEAM_1' THEN 1
                      WHEN 'TEAM_2' THEN 2
                  END,
                  CASE participant.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;

    let mut team_1 = Vec::with_capacity(5);
    let mut team_2 = Vec::with_capacity(5);
    for row in rows {
        let player = EntryConfirmationPlayerSnapshot {
            id: row.id,
            display_name: row.display_name,
            lane: row.lane,
        };
        match row.team.as_str() {
            "TEAM_1" => team_1.push(player),
            "TEAM_2" => team_2.push(player),
            _ => return Err(AttemptError::InvariantViolation),
        }
    }
    if team_1.len() != 5 || team_2.len() != 5 {
        return Err(AttemptError::InvariantViolation);
    }

    Ok(EntryConfirmationSnapshot {
        series: EntryConfirmationSeriesSnapshot {
            id: series_id,
            display_number,
            title: title.to_owned(),
            best_of,
            hard_fearless,
        },
        teams: vec![
            EntryConfirmationTeamSnapshot {
                team: "TEAM_1".to_owned(),
                name: "1팀".to_owned(),
                players: team_1,
            },
            EntryConfirmationTeamSnapshot {
                team: "TEAM_2".to_owned(),
                name: "2팀".to_owned(),
                players: team_2,
            },
        ],
        links: EntryConfirmationLinksSnapshot {
            pick_ban: format!("/g/{guild_id}/series/{series_id}/pick-ban"),
        },
    })
}

async fn enqueue_entry_confirmation_notification(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    channel_id: &DiscordChannelId,
    snapshot: &EntryConfirmationSnapshot,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let payload =
        serde_json::to_value(notification_outbox::EntryConfirmationNotificationPayload {
            schema_version: ENTRY_CONFIRMATION_SCHEMA_VERSION,
            series_id: snapshot.series.id.clone(),
            snapshot: serde_json::to_value(snapshot)?,
        })?;
    let deduplication_key = format!("series:{}:entry-confirmation", snapshot.series.id);
    notification_outbox::enqueue(
        transaction,
        &EnqueueNotification {
            id: notification_id,
            guild_id,
            deduplication_key: &deduplication_key,
            channel_id,
            notification_type: ENTRY_CONFIRMATION_CREATE_TYPE,
            payload: &payload,
            created_at: now,
        },
    )
    .await?;
    Ok(())
}

fn auction_match_lineup_confirmation_snapshot(
    guild_id: &GuildId,
    tournament: &AuctionTournamentLockRow,
    snapshot: &AuctionMatchSnapshot,
) -> Result<AuctionMatchLineupConfirmationSnapshot, AttemptError> {
    let left_team_id = snapshot.r#match.left_team_id;
    let right_team_id = snapshot.r#match.right_team_id;
    if snapshot.teams.len() != 2
        || snapshot
            .teams
            .iter()
            .map(|team| team.id)
            .collect::<HashSet<_>>()
            != HashSet::from([left_team_id, right_team_id])
        || !has_complete_auction_match_lineup(&snapshot.lineups, left_team_id, right_team_id)
    {
        return Err(AttemptError::InvariantViolation);
    }

    let mut teams = Vec::with_capacity(2);
    for team_id in [left_team_id, right_team_id] {
        let team = snapshot
            .teams
            .iter()
            .find(|team| team.id == team_id)
            .ok_or(AttemptError::InvariantViolation)?;
        if team.members.len() != 5 {
            return Err(AttemptError::InvariantViolation);
        }

        let mut players = Vec::with_capacity(5);
        for lane in [
            Lane::Top,
            Lane::Jungle,
            Lane::Mid,
            Lane::Bottom,
            Lane::Support,
        ] {
            let lineup = snapshot
                .lineups
                .iter()
                .find(|lineup| lineup.team_id == team_id && lineup.lane == lane)
                .ok_or(AttemptError::InvariantViolation)?;
            let member = team
                .members
                .iter()
                .find(|member| {
                    member.team_member_id == lineup.team_member_id
                        && member.user_id == lineup.user_id
                })
                .ok_or(AttemptError::InvariantViolation)?;
            players.push(AuctionMatchLineupConfirmationPlayerSnapshot {
                id: lineup.user_id.clone(),
                display_name: member.display_name.clone(),
                lane,
            });
        }
        teams.push(AuctionMatchLineupConfirmationTeamSnapshot {
            team_id,
            name: team.team_name.clone(),
            players,
        });
    }

    Ok(AuctionMatchLineupConfirmationSnapshot {
        tournament: AuctionMatchLineupConfirmationTournamentSnapshot {
            id: snapshot.r#match.tournament_id.clone(),
            display_number: tournament.display_number,
            title: tournament.title.clone(),
            format: parse_auction_format(&tournament.format)?,
            hard_fearless: tournament.hard_fearless,
        },
        matchup: AuctionMatchLineupConfirmationMatchSnapshot {
            id: snapshot.r#match.id.clone(),
            match_key: snapshot.r#match.match_key.clone(),
            label: snapshot.r#match.label.clone(),
            best_of: snapshot.r#match.best_of,
        },
        teams,
        links: AuctionMatchLineupConfirmationLinksSnapshot {
            pick_ban: format!("/g/{guild_id}/auction/matches/{}", snapshot.r#match.id),
        },
    })
}

async fn enqueue_auction_match_lineup_confirmation_notification(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    channel_id: &DiscordChannelId,
    snapshot: &AuctionMatchLineupConfirmationSnapshot,
    match_revision: i64,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let payload = serde_json::to_value(
        notification_outbox::AuctionMatchLineupConfirmationNotificationPayload {
            schema_version: AUCTION_MATCH_LINEUP_CONFIRMATION_SCHEMA_VERSION,
            match_id: snapshot.matchup.id.clone(),
            snapshot: serde_json::to_value(snapshot)?,
        },
    )?;
    let deduplication_key = format!(
        "auction-match:{}:lineup-confirmation:{match_revision}",
        snapshot.matchup.id
    );
    notification_outbox::enqueue(
        transaction,
        &EnqueueNotification {
            id: notification_id,
            guild_id,
            deduplication_key: &deduplication_key,
            channel_id,
            notification_type: AUCTION_MATCH_LINEUP_CONFIRMATION_CREATE_TYPE,
            payload: &payload,
            created_at: now,
        },
    )
    .await?;
    Ok(())
}

async fn enqueue_series_result_notification_if_configured(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
    series_revision: i64,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let Some(channel_id) = configured_series_result_channel(transaction, guild_id).await? else {
        return Ok(());
    };
    let snapshot = series_result_notification_snapshot(transaction, guild_id, series_id).await?;
    enqueue_series_result_notification(
        transaction,
        guild_id,
        &channel_id,
        &snapshot,
        series_revision,
        notification_id,
        now,
    )
    .await
}

async fn series_result_notification_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<SeriesResultNotificationSnapshot, AttemptError> {
    let series = sqlx::query_as::<_, SeriesResultNotificationSeriesRow>(
        "SELECT id, display_number, title, best_of, hard_fearless, winning_team, terminal_reason
         FROM series
         WHERE guild_id = $1
           AND id = $2
           AND status = 'COMPLETED'
           AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)?;
    let terminal_reason = series
        .terminal_reason
        .ok_or(AttemptError::InvariantViolation)?;
    let teams = series_result_notification_teams(transaction, guild_id, series_id).await?;
    let game_rows = sqlx::query_as::<_, SeriesResultNotificationGameRow>(
        "SELECT id, game_number AS set_number, side_preset, winning_side, patch_version
         FROM games
         WHERE guild_id = $1 AND series_id = $2 AND status = 'CONFIRMED'
         ORDER BY game_number",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let mut participants_by_game = game_rows
        .iter()
        .map(|game| (game.id, HashMap::new()))
        .collect::<HashMap<_, HashMap<_, _>>>();
    let participant_rows = sqlx::query_as::<_, SeriesResultNotificationGameParticipantRow>(
        "SELECT participant.game_id,
                participant.team,
                participant.user_id AS id,
                participant.lane,
                participant.champion_id
         FROM game_participants AS participant
         JOIN games AS game
           ON game.guild_id = participant.guild_id
          AND game.id = participant.game_id
         WHERE game.guild_id = $1
           AND game.series_id = $2
           AND game.status = 'CONFIRMED'
         ORDER BY game.game_number,
                  CASE participant.team
                      WHEN 'TEAM_1' THEN 1
                      WHEN 'TEAM_2' THEN 2
                  END,
                  CASE participant.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    for row in participant_rows {
        let participants = participants_by_game
            .get_mut(&row.game_id)
            .ok_or(AttemptError::InvariantViolation)?;
        if participants.insert(row.id.clone(), row).is_some() {
            return Err(AttemptError::InvariantViolation);
        }
    }

    let mut bans_by_game = game_rows
        .iter()
        .map(|game| {
            (
                game.id,
                SeriesResultNotificationGameBansSnapshot {
                    blue: vec![None; 5],
                    red: vec![None; 5],
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let ban_rows = sqlx::query_as::<_, SeriesResultNotificationGameBanRow>(
        "SELECT ban.game_id, ban.side, ban.slot, ban.champion_id
         FROM game_bans AS ban
         JOIN games AS game
           ON game.guild_id = ban.guild_id
          AND game.id = ban.game_id
         WHERE game.guild_id = $1
           AND game.series_id = $2
           AND game.status = 'CONFIRMED'
         ORDER BY game.game_number,
                  CASE ban.side
                      WHEN 'BLUE' THEN 1
                      WHEN 'RED' THEN 2
                  END,
                  ban.slot",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    for ban in ban_rows {
        let index = usize::try_from(
            ban.slot
                .checked_sub(1)
                .ok_or(AttemptError::InvariantViolation)?,
        )
        .map_err(|_| AttemptError::InvariantViolation)?;
        let bans = bans_by_game
            .get_mut(&ban.game_id)
            .ok_or(AttemptError::InvariantViolation)?;
        let slots = match ban.side.as_str() {
            "BLUE" => &mut bans.blue,
            "RED" => &mut bans.red,
            _ => return Err(AttemptError::InvariantViolation),
        };
        let slot = slots
            .get_mut(index)
            .ok_or(AttemptError::InvariantViolation)?;
        if slot.replace(ban.champion_id).is_some() {
            return Err(AttemptError::InvariantViolation);
        }
    }

    let team_1 = teams.first().ok_or(AttemptError::InvariantViolation)?;
    let team_2 = teams.get(1).ok_or(AttemptError::InvariantViolation)?;
    let mut games = Vec::with_capacity(game_rows.len());
    for game in game_rows {
        let winner = match winner_roster_team(&game.side_preset, &game.winning_side)? {
            RosterTeam::Team1 => "TEAM_1",
            RosterTeam::Team2 => "TEAM_2",
        };
        let side_preset = canonical_side_preset(&game.side_preset)?;
        let mut participants = participants_by_game
            .remove(&game.id)
            .ok_or(AttemptError::InvariantViolation)?;
        let team_1_game = series_result_notification_game_team(team_1, &mut participants)?;
        let team_2_game = series_result_notification_game_team(team_2, &mut participants)?;
        if !participants.is_empty() {
            return Err(AttemptError::InvariantViolation);
        }
        let bans = bans_by_game
            .remove(&game.id)
            .ok_or(AttemptError::InvariantViolation)?;
        games.push(SeriesResultNotificationGameSnapshot {
            set_number: game.set_number,
            winner: winner.to_owned(),
            side_preset,
            team_1: team_1_game,
            team_2: team_2_game,
            bans,
            patch_version: game.patch_version,
        });
    }
    if !participants_by_game.is_empty() || !bans_by_game.is_empty() {
        return Err(AttemptError::InvariantViolation);
    }
    let (team_1, team_2) = confirmed_series_score(transaction, guild_id, series_id).await?;

    Ok(SeriesResultNotificationSnapshot {
        series: SeriesResultNotificationSeriesSnapshot {
            id: SeriesId::from_uuid(series.id),
            display_number: series.display_number,
            title: series.title,
            best_of: series.best_of,
            hard_fearless: series.hard_fearless,
            winning_team: series.winning_team,
            terminal_reason,
        },
        teams,
        score: SeriesResultNotificationScoreSnapshot { team_1, team_2 },
        games,
        links: SeriesResultNotificationLinksSnapshot {
            result: format!("/g/{guild_id}/series/{series_id}/result"),
        },
    })
}

async fn series_result_notification_teams(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<Vec<SeriesResultNotificationTeamSnapshot>, AttemptError> {
    let rows = sqlx::query_as::<_, SeriesResultNotificationRosterRow>(
        "SELECT participant.team,
                participant.user_id AS id,
                participant.lane,
                member.display_name
         FROM series_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         WHERE participant.guild_id = $1
           AND participant.series_id = $2
         ORDER BY CASE participant.team
                      WHEN 'TEAM_1' THEN 1
                      WHEN 'TEAM_2' THEN 2
                  END,
                  CASE participant.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let mut team_1 = Vec::with_capacity(5);
    let mut team_2 = Vec::with_capacity(5);
    for row in rows {
        let player = SeriesResultNotificationPlayerSnapshot {
            id: row.id,
            display_name: row.display_name,
            lane: row.lane,
        };
        match row.team.as_str() {
            "TEAM_1" => team_1.push(player),
            "TEAM_2" => team_2.push(player),
            _ => return Err(AttemptError::InvariantViolation),
        }
    }
    if team_1.len() != 5 || team_2.len() != 5 {
        return Err(AttemptError::InvariantViolation);
    }
    Ok(vec![
        SeriesResultNotificationTeamSnapshot {
            team: "TEAM_1".to_owned(),
            name: "1팀".to_owned(),
            players: team_1,
        },
        SeriesResultNotificationTeamSnapshot {
            team: "TEAM_2".to_owned(),
            name: "2팀".to_owned(),
            players: team_2,
        },
    ])
}

fn series_result_notification_game_team(
    team: &SeriesResultNotificationTeamSnapshot,
    participants: &mut HashMap<UserId, SeriesResultNotificationGameParticipantRow>,
) -> Result<SeriesResultNotificationGameTeamSnapshot, AttemptError> {
    let mut picks = Vec::with_capacity(team.players.len());
    for player in &team.players {
        let participant = participants
            .remove(&player.id)
            .ok_or(AttemptError::InvariantViolation)?;
        if participant.team != team.team || participant.lane != player.lane {
            return Err(AttemptError::InvariantViolation);
        }
        picks.push(SeriesResultNotificationGamePickSnapshot {
            id: player.id.clone(),
            display_name: player.display_name.clone(),
            lane: player.lane,
            champion_id: participant.champion_id,
        });
    }
    Ok(SeriesResultNotificationGameTeamSnapshot {
        team: team.team.clone(),
        name: team.name.clone(),
        picks,
    })
}

async fn enqueue_series_result_notification(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    channel_id: &DiscordChannelId,
    snapshot: &SeriesResultNotificationSnapshot,
    series_revision: i64,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let payload = serde_json::to_value(notification_outbox::SeriesResultNotificationPayload {
        schema_version: SERIES_RESULT_SCHEMA_VERSION,
        series_id: snapshot.series.id.clone(),
        snapshot: serde_json::to_value(snapshot)?,
    })?;
    let deduplication_key = format!("series:{}:result:{series_revision}", snapshot.series.id);
    notification_outbox::enqueue(
        transaction,
        &EnqueueNotification {
            id: notification_id,
            guild_id,
            deduplication_key: &deduplication_key,
            channel_id,
            notification_type: SERIES_RESULT_CREATE_TYPE,
            payload: &payload,
            created_at: now,
        },
    )
    .await?;
    Ok(())
}

async fn enqueue_auction_tournament_result_notification_if_configured(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    tournament_revision: i64,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let Some(channel_id) = configured_series_result_channel(transaction, guild_id).await? else {
        return Ok(());
    };
    let payload = auction_tournament_result_notification_payload(
        transaction,
        guild_id,
        tournament_id,
        channel_id.clone(),
    )
    .await?;
    let value = serde_json::to_value(&payload)?;
    let deduplication_key =
        format!("auction-tournament:{tournament_id}:result:{tournament_revision}");
    notification_outbox::enqueue(
        transaction,
        &EnqueueNotification {
            id: notification_id,
            guild_id,
            deduplication_key: &deduplication_key,
            channel_id: &channel_id,
            notification_type: AUCTION_TOURNAMENT_RESULT_CREATE_TYPE,
            payload: &value,
            created_at: now,
        },
    )
    .await?;
    Ok(())
}

async fn reconcile_auction_tournament_result_notifications_after_revert(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    // Lock every prior result card before reopening the tournament. A live lease means the bot
    // may already be between Discord send and acknowledgement, so no safe correction exists yet.
    let rows = sqlx::query_as::<_, AuctionTournamentResultOutboxLockRow>(
        "SELECT id,
                channel_id,
                discord_message_id,
                delivered_at,
                failed_at,
                failure_code,
                attempts,
                (delivered_at IS NULL
                 AND failed_at IS NULL
                 AND lease_until >= clock_timestamp()) AS delivery_in_progress
         FROM bot_notification_outbox
         WHERE guild_id = $1
           AND notification_type = $2
           AND payload ->> 'tournamentId' = $3
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(AUCTION_TOURNAMENT_RESULT_CREATE_TYPE)
    .bind(tournament_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;

    if rows.iter().any(|row| row.delivery_in_progress) {
        return Err(AttemptError::AuctionTournamentResultDeliveryInProgress);
    }

    for row in rows {
        if row.delivered_at.is_some() {
            let discord_message_id = row
                .discord_message_id
                .ok_or(AttemptError::InvariantViolation)?;
            let payload = AuctionTournamentResultDeleteNotificationPayload {
                schema_version: AUCTION_TOURNAMENT_RESULT_DELETE_SCHEMA_VERSION,
                guild_id: guild_id.clone(),
                tournament_id,
                channel_id: row.channel_id.clone(),
                discord_message_id,
            };
            let value = serde_json::to_value(&payload)?;
            let deduplication_key = format!(
                "auction-tournament:{tournament_id}:result-delete:{}",
                row.id
            );
            notification_outbox::enqueue(
                transaction,
                &EnqueueNotification {
                    id: NotificationId::new(),
                    guild_id,
                    deduplication_key: &deduplication_key,
                    channel_id: &row.channel_id,
                    notification_type: AUCTION_TOURNAMENT_RESULT_DELETE_TYPE,
                    payload: &value,
                    created_at: now,
                },
            )
            .await?;
        } else if row.failed_at.is_none() && row.attempts == 0 {
            let cancelled = sqlx::query(
                "UPDATE bot_notification_outbox
                 SET failed_at = $1,
                     failure_code = 'RESULT_REVERTED',
                     last_error = 'auction tournament result was reverted before delivery',
                     lease_owner = NULL,
                     lease_token = NULL,
                     lease_until = NULL
                 WHERE id = $2 AND delivered_at IS NULL AND failed_at IS NULL",
            )
            .bind(now)
            .bind(row.id)
            .execute(&mut **transaction)
            .await?;
            if cancelled.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
        } else if row.failure_code.as_deref() != Some("RESULT_REVERTED") {
            // Once a create has been leased, a Discord send may have happened even when the
            // acknowledgement did not. Never reopen the tournament while that card is unknown.
            return Err(AttemptError::AuctionTournamentResultDeliveryUncertain);
        }
    }

    Ok(())
}

async fn auction_tournament_result_notification_payload(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    channel_id: DiscordChannelId,
) -> Result<AuctionTournamentResultNotificationPayload, AttemptError> {
    let tournament = sqlx::query_as::<_, AuctionTournamentResultNotificationRow>(
        "SELECT tournament.display_number, tournament.title, tournament.format,
                tournament.semifinal_best_of, tournament.final_best_of, tournament.hard_fearless,
                tournament.champion_team_id, tournament.ended_at, champion.team_name AS champion_team_name
         FROM auction_tournaments AS tournament
         JOIN auction_teams AS champion
           ON champion.guild_id = tournament.guild_id
          AND champion.tournament_id = tournament.id
          AND champion.id = tournament.champion_team_id
         WHERE tournament.guild_id = $1
           AND tournament.id = $2
           AND tournament.status = 'COMPLETED'
           AND tournament.deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)?;
    let format = parse_auction_format(&tournament.format)?;
    let rules = AuctionRecruitmentSeriesRules {
        hard_fearless: tournament.hard_fearless,
        semifinal_best_of: tournament.semifinal_best_of,
        final_best_of: tournament.final_best_of,
    };
    validate_auction_recruitment_series_rules(format, &rules)
        .map_err(|_| AttemptError::InvariantViolation)?;
    let rows = sqlx::query_as::<_, AuctionTournamentResultNotificationMatchRow>(
        "SELECT auction_match.match_key,
                auction_match.best_of,
                left_team.team_name AS left_team_name,
                right_team.team_name AS right_team_name,
                winner_team.team_name AS winner_team_name,
                count(game.id) FILTER (
                    WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE')
                       OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                ) AS left_score,
                count(game.id) FILTER (
                    WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED')
                       OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                ) AS right_score
         FROM auction_matches AS auction_match
         JOIN auction_teams AS left_team
           ON left_team.guild_id = auction_match.guild_id
          AND left_team.tournament_id = auction_match.tournament_id
          AND left_team.id = auction_match.left_team_id
         JOIN auction_teams AS right_team
           ON right_team.guild_id = auction_match.guild_id
          AND right_team.tournament_id = auction_match.tournament_id
          AND right_team.id = auction_match.right_team_id
         JOIN auction_teams AS winner_team
           ON winner_team.guild_id = auction_match.guild_id
          AND winner_team.tournament_id = auction_match.tournament_id
          AND winner_team.id = auction_match.winner_team_id
         LEFT JOIN games AS game
           ON game.guild_id = auction_match.guild_id
          AND game.auction_match_id = auction_match.id
          AND game.status = 'CONFIRMED'
         WHERE auction_match.guild_id = $1
           AND auction_match.tournament_id = $2
           AND auction_match.status = 'COMPLETED'
           AND auction_match.deleted_at IS NULL
         GROUP BY auction_match.match_key, auction_match.best_of,
                  left_team.team_name, right_team.team_name, winner_team.team_name
         ORDER BY CASE auction_match.match_key
                      WHEN 'SEMIFINAL_A' THEN 1
                      WHEN 'SEMIFINAL_B' THEN 2
                      WHEN 'FINAL' THEN 3
                      ELSE 4
                  END",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let expected_keys: &[&str] = match format {
        AuctionFormat::TenPerson => &["FINAL"],
        AuctionFormat::TwentyPerson => &["SEMIFINAL_A", "SEMIFINAL_B", "FINAL"],
    };
    if rows.len() != expected_keys.len()
        || rows
            .iter()
            .map(|row| row.match_key.as_str())
            .ne(expected_keys.iter().copied())
    {
        return Err(AttemptError::InvariantViolation);
    }
    let mut matches = Vec::with_capacity(rows.len());
    for row in rows {
        let required_wins = i64::from(row.best_of / 2 + 1);
        let winner_score = if row.winner_team_name == row.left_team_name {
            row.left_score
        } else if row.winner_team_name == row.right_team_name {
            row.right_score
        } else {
            return Err(AttemptError::InvariantViolation);
        };
        if winner_score != required_wins
            || row.left_score + row.right_score > i64::from(row.best_of)
        {
            return Err(AttemptError::InvariantViolation);
        }
        matches.push(AuctionTournamentResultNotificationMatch {
            label: auction_match_label(&row.match_key)?.to_owned(),
            match_key: row.match_key,
            best_of: row.best_of,
            left_team_name: row.left_team_name,
            right_team_name: row.right_team_name,
            left_score: row.left_score,
            right_score: row.right_score,
            winner_team_name: row.winner_team_name,
        });
    }
    let champion_team_id = tournament
        .champion_team_id
        .ok_or(AttemptError::InvariantViolation)?;
    let completed_at = tournament
        .ended_at
        .ok_or(AttemptError::InvariantViolation)?;
    let final_match = matches.last().ok_or(AttemptError::InvariantViolation)?;
    if final_match.winner_team_name != tournament.champion_team_name {
        return Err(AttemptError::InvariantViolation);
    }
    Ok(AuctionTournamentResultNotificationPayload {
        schema_version: AUCTION_TOURNAMENT_RESULT_SCHEMA_VERSION,
        guild_id: guild_id.clone(),
        tournament_id,
        channel_id,
        tournament: AuctionTournamentResultNotificationTournament {
            display_number: tournament.display_number,
            title: tournament.title,
            format,
            semifinal_best_of: rules.semifinal_best_of,
            final_best_of: rules.final_best_of,
            hard_fearless: rules.hard_fearless,
        },
        champion: AuctionTournamentResultNotificationChampion {
            team_id: champion_team_id,
            team_name: tournament.champion_team_name,
        },
        matches,
        completed_at,
    })
}

fn direct_message_recipient(
    user_id: &UserId,
) -> Result<Option<(DiscordUserId, DiscordChannelId)>, AttemptError> {
    let Ok(discord_user_id) = DiscordUserId::try_from(user_id.clone()) else {
        // Development dummy users have no Discord identity and must never receive a DM.
        return Ok(None);
    };
    let routing_id = DiscordChannelId::parse(discord_user_id.as_str())
        .map_err(|_| AttemptError::InvariantViolation)?;
    Ok(Some((discord_user_id, routing_id)))
}

async fn enqueue_recruitment_participant_confirmation_direct_messages(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    snapshot: &RecruitmentSnapshot,
    now: OffsetDateTime,
) -> Result<ParticipantConfirmationDmDispatchResult, AttemptError> {
    let mut result = ParticipantConfirmationDmDispatchResult::default();
    // Recruitment snapshots contain only the canonical JOINED roster.
    for participant in &snapshot.participants {
        let Some((recipient_user_id, routing_id)) = direct_message_recipient(&participant.id)?
        else {
            continue;
        };
        let deduplication_key = format!(
            "recruitment:{}:participant-confirmation-dm:{}:{}",
            snapshot.recruitment.id, snapshot.recruitment.revision, recipient_user_id
        );
        let payload = serde_json::to_value(
            notification_outbox::RecruitmentParticipantConfirmationDmPayload {
                schema_version: RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_SCHEMA_VERSION,
                guild_id: guild_id.clone(),
                recipient_user_id,
                recruitment_id: snapshot.recruitment.id.as_uuid(),
                recruitment_title: snapshot.recruitment.title.clone(),
                recruitment_href: snapshot.links.entry.clone(),
            },
        )?;
        match notification_outbox::enqueue(
            transaction,
            &EnqueueNotification {
                id: NotificationId::new(),
                guild_id,
                deduplication_key: &deduplication_key,
                channel_id: &routing_id,
                notification_type: RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE_TYPE,
                payload: &payload,
                created_at: now,
            },
        )
        .await?
        {
            EnqueueNotificationOutcome::Enqueued(_) => result.queued_recipient_count += 1,
            EnqueueNotificationOutcome::Existing(_) => result.already_queued_recipient_count += 1,
        }
    }
    Ok(result)
}

async fn enqueue_auction_recruitment_participant_confirmation_direct_messages(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    snapshot: &AuctionRecruitmentSnapshot,
    now: OffsetDateTime,
) -> Result<ParticipantConfirmationDmDispatchResult, AttemptError> {
    let mut result = ParticipantConfirmationDmDispatchResult::default();
    let recruitment_href = snapshot
        .links
        .draft
        .as_deref()
        .unwrap_or(&snapshot.recruitment.href);
    // Auction recruitment snapshots contain only the canonical JOINED roster.
    for participant in &snapshot.participants {
        let Some((recipient_user_id, routing_id)) = direct_message_recipient(&participant.id)?
        else {
            continue;
        };
        let deduplication_key = format!(
            "auction-recruitment:{}:participant-confirmation-dm:{}:{}",
            snapshot.recruitment.id, snapshot.recruitment.revision, recipient_user_id
        );
        let payload = serde_json::to_value(
            notification_outbox::RecruitmentParticipantConfirmationDmPayload {
                schema_version: RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_SCHEMA_VERSION,
                guild_id: guild_id.clone(),
                recipient_user_id,
                recruitment_id: snapshot.recruitment.id.as_uuid(),
                recruitment_title: snapshot.recruitment.title.clone(),
                recruitment_href: recruitment_href.to_owned(),
            },
        )?;
        match notification_outbox::enqueue(
            transaction,
            &EnqueueNotification {
                id: NotificationId::new(),
                guild_id,
                deduplication_key: &deduplication_key,
                channel_id: &routing_id,
                notification_type: AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE_TYPE,
                payload: &payload,
                created_at: now,
            },
        )
        .await?
        {
            EnqueueNotificationOutcome::Enqueued(_) => result.queued_recipient_count += 1,
            EnqueueNotificationOutcome::Existing(_) => result.already_queued_recipient_count += 1,
        }
    }
    Ok(result)
}

async fn enqueue_web_recruitment_message_notification(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    snapshot: &RecruitmentSnapshot,
    command: &WebRecruitmentMutation,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let (Some(channel_id), Some(_message_id)) = (
        snapshot.recruitment.channel_id.as_ref(),
        snapshot.recruitment.message_id.as_ref(),
    ) else {
        return Ok(());
    };
    let (notification_type, deduplication_key) = match command {
        WebRecruitmentMutation::AddMember { .. }
        | WebRecruitmentMutation::RemoveMember { .. }
        | WebRecruitmentMutation::Close { .. } => (
            RECRUITMENT_MESSAGE_UPDATE_TYPE,
            format!(
                "recruitment:{}:message:update:{}",
                snapshot.recruitment.id, snapshot.recruitment.revision
            ),
        ),
        WebRecruitmentMutation::Cancel { .. } => (
            RECRUITMENT_MESSAGE_DELETE_TYPE,
            format!("recruitment:{}:message:delete", snapshot.recruitment.id),
        ),
    };
    enqueue_recruitment_message_notification(
        transaction,
        guild_id,
        channel_id,
        snapshot,
        notification_type,
        deduplication_key,
        notification_id,
        now,
    )
    .await
}

async fn enqueue_recruitment_message_update(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    snapshot: &RecruitmentSnapshot,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let (Some(channel_id), Some(_message_id)) = (
        snapshot.recruitment.channel_id.as_ref(),
        snapshot.recruitment.message_id.as_ref(),
    ) else {
        return Ok(());
    };
    enqueue_recruitment_message_notification(
        transaction,
        guild_id,
        channel_id,
        snapshot,
        RECRUITMENT_MESSAGE_UPDATE_TYPE,
        format!(
            "recruitment:{}:message:update:{}",
            snapshot.recruitment.id, snapshot.recruitment.revision
        ),
        notification_id,
        now,
    )
    .await
}

async fn enqueue_recruitment_message_notification(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    channel_id: &DiscordChannelId,
    snapshot: &RecruitmentSnapshot,
    notification_type: &'static str,
    deduplication_key: String,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let payload =
        serde_json::to_value(notification_outbox::RecruitmentMessageNotificationPayload {
            schema_version: RECRUITMENT_MESSAGE_SCHEMA_VERSION,
            recruitment_id: snapshot.recruitment.id,
            snapshot: serde_json::to_value(snapshot)?,
        })?;
    notification_outbox::enqueue(
        transaction,
        &EnqueueNotification {
            id: notification_id,
            guild_id,
            deduplication_key: &deduplication_key,
            channel_id,
            notification_type,
            payload: &payload,
            created_at: now,
        },
    )
    .await?;
    Ok(())
}

async fn enqueue_auction_recruitment_message_lifecycle_notification(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    snapshot: &AuctionRecruitmentSnapshot,
    command: &WebAuctionRecruitmentLifecycle,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let (Some(channel_id), Some(_message_id)) = (
        snapshot.recruitment.channel_id.as_ref(),
        snapshot.recruitment.message_id.as_ref(),
    ) else {
        return Ok(());
    };
    let (notification_type, deduplication_key) = match command {
        WebAuctionRecruitmentLifecycle::Close { .. } => (
            AUCTION_RECRUITMENT_MESSAGE_UPDATE_TYPE,
            format!(
                "auction-recruitment:{}:message:update:{}",
                snapshot.recruitment.id, snapshot.recruitment.revision
            ),
        ),
        WebAuctionRecruitmentLifecycle::Cancel { .. } => (
            AUCTION_RECRUITMENT_MESSAGE_DELETE_TYPE,
            format!(
                "auction-recruitment:{}:message:delete",
                snapshot.recruitment.id
            ),
        ),
    };
    enqueue_auction_recruitment_message_notification(
        transaction,
        guild_id,
        channel_id,
        snapshot,
        notification_type,
        deduplication_key,
        notification_id,
        now,
    )
    .await
}

async fn enqueue_auction_recruitment_message_update(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    snapshot: &AuctionRecruitmentSnapshot,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let (Some(channel_id), Some(_message_id)) = (
        snapshot.recruitment.channel_id.as_ref(),
        snapshot.recruitment.message_id.as_ref(),
    ) else {
        return Ok(());
    };
    enqueue_auction_recruitment_message_notification(
        transaction,
        guild_id,
        channel_id,
        snapshot,
        AUCTION_RECRUITMENT_MESSAGE_UPDATE_TYPE,
        format!(
            "auction-recruitment:{}:message:update:{}",
            snapshot.recruitment.id, snapshot.recruitment.revision
        ),
        notification_id,
        now,
    )
    .await
}

async fn enqueue_auction_recruitment_message_notification(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    channel_id: &DiscordChannelId,
    snapshot: &AuctionRecruitmentSnapshot,
    notification_type: &'static str,
    deduplication_key: String,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let payload = serde_json::to_value(
        notification_outbox::AuctionRecruitmentMessageNotificationPayload {
            schema_version: AUCTION_RECRUITMENT_MESSAGE_SCHEMA_VERSION,
            auction_recruitment_id: snapshot.recruitment.id,
            snapshot: serde_json::to_value(snapshot)?,
        },
    )?;
    notification_outbox::enqueue(
        transaction,
        &EnqueueNotification {
            id: notification_id,
            guild_id,
            deduplication_key: &deduplication_key,
            channel_id,
            notification_type,
            payload: &payload,
            created_at: now,
        },
    )
    .await?;
    Ok(())
}

async fn require_active_session_operator(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    require_active_member_for_creation(transaction, guild_id, actor_id).await?;
    require_session_operator_role(transaction, guild_id, actor_id, operator_role_id).await
}

async fn require_active_session_member_read(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    user_id: &UserId,
) -> Result<(), AttemptError> {
    let active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM guild_members AS member
             JOIN guilds AS guild ON guild.guild_id = member.guild_id
             WHERE member.guild_id = $1
               AND member.user_id = $2
               AND member.status = 'ACTIVE'
               AND guild.status = 'ACTIVE'
         )",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(&mut **transaction)
    .await?;
    if active {
        Ok(())
    } else {
        Err(AttemptError::MembershipRequired)
    }
}

async fn require_active_creator_or_operator(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    created_by: &UserId,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    require_active_member_for_creation(transaction, guild_id, actor_id).await?;
    if actor_id == created_by {
        return Ok(());
    }
    require_session_operator_role(transaction, guild_id, actor_id, operator_role_id).await
}

async fn require_session_operator_role(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    let has_role = sqlx::query_scalar::<_, bool>(
        "SELECT true
         FROM guild_member_discord_roles
         WHERE guild_id = $1 AND user_id = $2 AND role_id = $3
         FOR SHARE",
    )
    .bind(guild_id)
    .bind(actor_id)
    .bind(operator_role_id)
    .fetch_optional(&mut **transaction)
    .await?
    .is_some();
    if has_role {
        Ok(())
    } else {
        Err(AttemptError::OperatorRequired)
    }
}

async fn is_active_session_operator(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    user_id: &UserId,
) -> Result<bool, AttemptError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM guild_members AS member
             JOIN guild_config AS config
               ON config.guild_id = member.guild_id
             JOIN guild_member_discord_roles AS role
               ON role.guild_id = member.guild_id
              AND role.user_id = member.user_id
              AND role.role_id = config.operator_role_id
             WHERE member.guild_id = $1
               AND member.user_id = $2
               AND member.status = 'ACTIVE'
         )",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(&mut **transaction)
    .await?)
}

async fn lock_active_guild(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<DiscordRoleId, AttemptError> {
    let status = sqlx::query_scalar::<_, GuildStatus>(
        "SELECT status
         FROM guilds
         WHERE guild_id = $1
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::GuildNotFound)?;
    if status != GuildStatus::Active {
        return Err(AttemptError::GuildInactive(status));
    }
    sqlx::query_scalar::<_, DiscordRoleId>(
        "SELECT operator_role_id
         FROM guild_config
         WHERE guild_id = $1
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::GuildNotConfigured)
}

async fn lock_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<RecruitmentLockRow, AttemptError> {
    sqlx::query_as::<_, RecruitmentLockRow>(
        "SELECT season_id, status, target_count, best_of, hard_fearless, revision, created_by
         FROM recruitments
         WHERE guild_id = $1 AND id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::RecruitmentNotFound)
}

async fn lock_auction_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
) -> Result<AuctionRecruitmentLockRow, AttemptError> {
    sqlx::query_as::<_, AuctionRecruitmentLockRow>(
        "SELECT season_id, format, status, target_count, semifinal_best_of, final_best_of,
                hard_fearless, revision, created_by
         FROM auction_recruitments
         WHERE guild_id = $1 AND id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionRecruitmentNotFound)
}

async fn lock_auction_setup_draft_revision(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "SELECT revision
         FROM auction_setup_drafts
         WHERE guild_id = $1 AND recruitment_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingAuctionSetupDraft)
}

async fn lock_auction_setup_teams(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
) -> Result<Vec<AuctionSetupTeamRow>, AttemptError> {
    Ok(sqlx::query_as::<_, AuctionSetupTeamRow>(
        "SELECT team_index, team_name, captain_user_id, initial_points
         FROM auction_setup_teams
         WHERE guild_id = $1 AND recruitment_id = $2
         ORDER BY team_index
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn lock_joined_auction_recruitment_participants(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
) -> Result<Vec<AuctionRecruitmentParticipantLockRow>, AttemptError> {
    Ok(sqlx::query_as::<_, AuctionRecruitmentParticipantLockRow>(
        "SELECT user_id, declared_lane, mmr_at_join, snapshot_origin
         FROM auction_recruitment_participants
         WHERE guild_id = $1 AND recruitment_id = $2 AND status = 'JOINED'
         ORDER BY joined_at, user_id
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn lock_auction_tournament(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<AuctionTournamentLockRow, AttemptError> {
    sqlx::query_as::<_, AuctionTournamentLockRow>(
        "SELECT display_number, title, format, hard_fearless, status, revision, bracket_locked_at
         FROM auction_tournaments
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)
}

async fn lock_auction_tournament_for_cancel(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<AuctionTournamentCancelLockRow, AttemptError> {
    sqlx::query_as::<_, AuctionTournamentCancelLockRow>(
        "SELECT source_recruitment_id, status, revision
         FROM auction_tournaments
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)
}

async fn auction_tournament_id_for_source_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
) -> Result<AuctionTournamentId, AttemptError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM auction_tournaments
         WHERE guild_id = $1
           AND source_recruitment_id = $2
           AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .map(AuctionTournamentId::from_uuid)
    .ok_or(AttemptError::AuctionTournamentNotFound)
}

async fn lock_auction_tournament_bracket(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<AuctionTournamentBracketLockRow, AttemptError> {
    sqlx::query_as::<_, AuctionTournamentBracketLockRow>(
        "SELECT season_id, format, semifinal_best_of, final_best_of, hard_fearless,
                status, revision, bracket_locked_at
         FROM auction_tournaments
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)
}

async fn ensure_auction_bracket_slot_rows(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    for slot_key in AUCTION_BRACKET_SLOT_KEYS {
        sqlx::query(
            "INSERT INTO auction_bracket_slots (
                 guild_id, tournament_id, slot_key, team_id, updated_by, updated_at
             ) VALUES ($1, $2, $3, NULL, NULL, $4)
             ON CONFLICT (guild_id, tournament_id, slot_key) DO NOTHING",
        )
        .bind(guild_id)
        .bind(tournament_id.as_uuid())
        .bind(slot_key)
        .bind(now)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn lock_auction_tournament_bracket_slots(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<Vec<AuctionTournamentBracketSlotRow>, AttemptError> {
    Ok(sqlx::query_as::<_, AuctionTournamentBracketSlotRow>(
        "SELECT slot_key, team_id
         FROM auction_bracket_slots
         WHERE guild_id = $1 AND tournament_id = $2
         ORDER BY CASE slot_key
                      WHEN 'A_0' THEN 0
                      WHEN 'A_1' THEN 1
                      WHEN 'B_0' THEN 2
                      WHEN 'B_1' THEN 3
                      ELSE 4
                  END
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn lock_auction_tournament_matches(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<Vec<Uuid>, AttemptError> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM auction_matches
         WHERE guild_id = $1
           AND tournament_id = $2
           AND deleted_at IS NULL
           AND status <> 'CANCELLED'
         ORDER BY id
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn auction_match_tournament_id(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
) -> Result<AuctionTournamentId, AttemptError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT tournament_id
         FROM auction_matches
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .map(AuctionTournamentId::from_uuid)
    .ok_or(AttemptError::AuctionMatchNotFound)
}

async fn lock_auction_match(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
) -> Result<AuctionMatchLockRow, AttemptError> {
    sqlx::query_as::<_, AuctionMatchLockRow>(
        "SELECT tournament_id, season_id, match_key, best_of, hard_fearless,
                status, revision, left_team_id, right_team_id, winner_team_id,
                lineups_locked_at, lineups_locked_by
         FROM auction_matches
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionMatchNotFound)
}

async fn lock_auction_match_lineups(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    match_id: AuctionMatchId,
) -> Result<Vec<AuctionMatchLineupRow>, AttemptError> {
    Ok(sqlx::query_as::<_, AuctionMatchLineupRow>(
        "SELECT team_id, lane, team_member_id, user_id
         FROM auction_match_lineups
         WHERE guild_id = $1 AND tournament_id = $2 AND match_id = $3
         ORDER BY team_id, lane
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(match_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn bump_auction_match_lineup_revision(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE auction_matches
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE guild_id = $3
           AND id = $4
           AND status = 'LINEUP_PENDING'
           AND lineups_locked_at IS NULL
           AND deleted_at IS NULL
         RETURNING revision",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionMatchLineupLocked)
}

async fn lock_auction_match_lineup(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE auction_matches
         SET status = 'READY',
             lineups_locked_at = $1,
             lineups_locked_by = $2,
             revision = revision + 1,
             updated_by = $2,
             updated_at = $1
         WHERE guild_id = $3
           AND id = $4
           AND status = 'LINEUP_PENDING'
           AND lineups_locked_at IS NULL
           AND deleted_at IS NULL
         RETURNING revision",
    )
    .bind(now)
    .bind(actor_id)
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionMatchLineupLocked)
}

async fn unlock_auction_match_lineup(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE auction_matches
         SET status = 'LINEUP_PENDING',
             lineups_locked_at = NULL,
             lineups_locked_by = NULL,
             revision = revision + 1,
             updated_by = $1,
             updated_at = $2
         WHERE guild_id = $3
           AND id = $4
           AND status = 'READY'
           AND lineups_locked_at IS NOT NULL
           AND lineups_locked_by IS NOT NULL
           AND deleted_at IS NULL
         RETURNING revision",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionMatchLineupUnlockUnavailable(
        "READY".to_owned(),
    ))
}

async fn downgrade_auction_tournament_after_lineup_unlock(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    // The tournament only returns to bracket setup when no other active match keeps a lock or
    // result artifact. The tournament row is already locked by the caller.
    sqlx::query(
        "UPDATE auction_tournaments AS tournament
         SET status = 'BRACKET_SETUP',
             revision = tournament.revision + 1,
             updated_by = $1,
             updated_at = $2
         WHERE tournament.guild_id = $3
           AND tournament.id = $4
           AND tournament.status = 'IN_GAME'
           AND tournament.deleted_at IS NULL
           AND NOT EXISTS (
                SELECT 1
                FROM auction_matches AS auction_match
                WHERE auction_match.guild_id = tournament.guild_id
                  AND auction_match.tournament_id = tournament.id
                  AND auction_match.deleted_at IS NULL
                  AND auction_match.status <> 'CANCELLED'
                  AND (
                      auction_match.lineups_locked_at IS NOT NULL
                      OR EXISTS (
                          SELECT 1
                          FROM games
                          WHERE games.guild_id = auction_match.guild_id
                            AND games.auction_match_id = auction_match.id
                      )
                      OR EXISTS (
                          SELECT 1
                          FROM game_inputs
                          WHERE game_inputs.guild_id = auction_match.guild_id
                            AND game_inputs.auction_match_id = auction_match.id
                      )
                  )
           )",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn advance_auction_tournament_for_lineup_lock(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let updated = sqlx::query(
        "UPDATE auction_tournaments
         SET status = CASE WHEN status = 'BRACKET_SETUP' THEN 'IN_GAME' ELSE status END,
             revision = revision + 1,
             updated_by = $1,
             updated_at = $2
         WHERE guild_id = $3
           AND id = $4
           AND status IN ('BRACKET_SETUP', 'IN_GAME')
           AND deleted_at IS NULL",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AttemptError::AuctionMatchTournamentNotReady)
    }
}

fn ensure_auction_match_result_editable(
    auction_match: &AuctionMatchLockRow,
    command: &WebAuctionMatchGameMutation,
) -> Result<(), AttemptError> {
    let allowed = match command {
        WebAuctionMatchGameMutation::Submit(_) => {
            matches!(auction_match.status.as_str(), "READY" | "IN_PROGRESS")
        }
        WebAuctionMatchGameMutation::Revert(_) => matches!(
            auction_match.status.as_str(),
            "READY" | "IN_PROGRESS" | "COMPLETED"
        ),
    };
    if allowed {
        Ok(())
    } else {
        Err(AttemptError::AuctionMatchResultNotEditable(
            auction_match.status.clone(),
        ))
    }
}

fn next_auction_match_set_number(
    confirmed_game_count: i64,
    best_of: i16,
) -> Result<i16, AttemptError> {
    let next =
        i16::try_from(confirmed_game_count + 1).map_err(|_| AttemptError::InvariantViolation)?;
    if next < 1 || next > best_of {
        return Err(AttemptError::AuctionMatchResultNotEditable(
            "COMPLETED".to_owned(),
        ));
    }
    Ok(next)
}

async fn update_auction_match_result_state(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    now: OffsetDateTime,
    winner_team_id: Option<Uuid>,
) -> Result<i64, AttemptError> {
    let (status, ended_at) = match winner_team_id {
        Some(_) => ("COMPLETED", Some(now)),
        None => ("IN_PROGRESS", None),
    };
    sqlx::query_scalar::<_, i64>(
        "UPDATE auction_matches
         SET status = $1,
             winner_team_id = $2,
             started_at = COALESCE(started_at, $3),
             ended_at = $4,
             revision = revision + 1,
             updated_by = $5,
             updated_at = $3
         WHERE guild_id = $6
           AND id = $7
           AND status IN ('READY', 'IN_PROGRESS')
           AND lineups_locked_at IS NOT NULL
           AND deleted_at IS NULL
         RETURNING revision",
    )
    .bind(status)
    .bind(winner_team_id)
    .bind(now)
    .bind(ended_at)
    .bind(actor_id)
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionMatchResultNotEditable(
        status.to_owned(),
    ))
}

async fn reopen_auction_match_after_game_revert(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    now: OffsetDateTime,
    confirmed_game_count: i64,
) -> Result<i64, AttemptError> {
    let status = if confirmed_game_count == 0 {
        "READY"
    } else {
        "IN_PROGRESS"
    };
    sqlx::query_scalar::<_, i64>(
        "UPDATE auction_matches
         SET status = $1,
             winner_team_id = NULL,
             ended_at = NULL,
             revision = revision + 1,
             updated_by = $2,
             updated_at = $3
         WHERE guild_id = $4
           AND id = $5
           AND status IN ('READY', 'IN_PROGRESS', 'COMPLETED')
           AND lineups_locked_at IS NOT NULL
           AND deleted_at IS NULL
         RETURNING revision",
    )
    .bind(status)
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)
}

async fn lock_auction_match_by_key(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    match_key: &str,
) -> Result<(AuctionMatchId, AuctionMatchLockRow), AttemptError> {
    let row = sqlx::query_as::<_, AuctionMatchByKeyLockRow>(
        "SELECT id, tournament_id, season_id, match_key, best_of, hard_fearless,
                status, revision, left_team_id, right_team_id, winner_team_id,
                lineups_locked_at, lineups_locked_by
         FROM auction_matches
         WHERE guild_id = $1
           AND tournament_id = $2
           AND match_key = $3
           AND status <> 'CANCELLED'
           AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(match_key)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)?;
    Ok((
        AuctionMatchId::from_uuid(row.id),
        AuctionMatchLockRow {
            tournament_id: row.tournament_id,
            season_id: row.season_id,
            match_key: row.match_key,
            best_of: row.best_of,
            hard_fearless: row.hard_fearless,
            status: row.status,
            revision: row.revision,
            left_team_id: row.left_team_id,
            right_team_id: row.right_team_id,
            winner_team_id: row.winner_team_id,
            lineups_locked_at: row.lineups_locked_at,
            lineups_locked_by: row.lineups_locked_by,
        },
    ))
}

async fn advance_auction_tournament_after_match_completion(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    match_id: AuctionMatchId,
    auction_match: &AuctionMatchLockRow,
    winner_team_id: Uuid,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<Option<i64>, AttemptError> {
    let tournament = sqlx::query_as::<_, AuctionTournamentCompletionLockRow>(
        "SELECT format, status
         FROM auction_tournaments
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)?;
    let format = parse_auction_format(&tournament.format)?;
    match (format, auction_match.match_key.as_str()) {
        (AuctionFormat::TwentyPerson, "SEMIFINAL_A")
        | (AuctionFormat::TwentyPerson, "SEMIFINAL_B") => {
            if tournament.status != AuctionTournamentStatus::InGame {
                return Err(AttemptError::AuctionMatchTournamentNotReady);
            }
            let final_side = if auction_match.match_key == "SEMIFINAL_A" {
                "LEFT"
            } else {
                "RIGHT"
            };
            propagate_auction_semifinal_winner_to_final(
                transaction,
                guild_id,
                tournament_id,
                match_id,
                auction_match,
                winner_team_id,
                final_side,
                actor_id,
                now,
            )
            .await?;
            Ok(None)
        }
        (AuctionFormat::TenPerson, "FINAL") | (AuctionFormat::TwentyPerson, "FINAL") => {
            let revision = sqlx::query_scalar::<_, i64>(
                "UPDATE auction_tournaments
                 SET status = 'COMPLETED',
                     champion_team_id = $1,
                     ended_at = $2,
                     revision = revision + 1,
                     updated_by = $3,
                     updated_at = $2
                 WHERE guild_id = $4
                   AND id = $5
                   AND status = 'IN_GAME'
                   AND deleted_at IS NULL
                 RETURNING revision",
            )
            .bind(winner_team_id)
            .bind(now)
            .bind(actor_id)
            .bind(guild_id)
            .bind(tournament_id.as_uuid())
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AttemptError::AuctionMatchTournamentNotReady)?;
            Ok(Some(revision))
        }
        _ => Err(AttemptError::InvariantViolation),
    }
}

#[allow(clippy::too_many_arguments)]
async fn propagate_auction_semifinal_winner_to_final(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    semifinal_match_id: AuctionMatchId,
    semifinal: &AuctionMatchLockRow,
    winner_team_id: Uuid,
    final_side: &str,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    if semifinal.lineups_locked_at.is_none()
        || !matches!(semifinal.match_key.as_str(), "SEMIFINAL_A" | "SEMIFINAL_B")
    {
        return Err(AttemptError::InvariantViolation);
    }
    let (final_match_id, final_match) =
        lock_auction_match_by_key(transaction, guild_id, tournament_id, "FINAL").await?;
    if final_match.status != "LINEUP_PENDING" || final_match.lineups_locked_at.is_some() {
        return Err(AttemptError::AuctionMatchDownstreamWorkExists);
    }
    let existing_target_team = match final_side {
        "LEFT" => final_match.left_team_id,
        "RIGHT" => final_match.right_team_id,
        _ => return Err(AttemptError::InvariantViolation),
    };
    if existing_target_team.is_some() {
        return Err(AttemptError::AuctionMatchDownstreamWorkExists);
    }
    let target_lineup_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM auction_match_lineups
         WHERE guild_id = $1 AND tournament_id = $2 AND match_id = $3 AND team_id = $4",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(final_match_id.as_uuid())
    .bind(winner_team_id)
    .fetch_one(&mut **transaction)
    .await?;
    if target_lineup_count != 0 {
        return Err(AttemptError::AuctionMatchDownstreamWorkExists);
    }
    let source_lineups = sqlx::query_as::<_, AuctionMatchLineupRow>(
        "SELECT team_id, lane, team_member_id, user_id
         FROM auction_match_lineups
         WHERE guild_id = $1 AND tournament_id = $2 AND match_id = $3 AND team_id = $4
         ORDER BY lane
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(semifinal_match_id.as_uuid())
    .bind(winner_team_id)
    .fetch_all(&mut **transaction)
    .await?;
    if source_lineups.len() != 5 {
        return Err(AttemptError::InvariantViolation);
    }
    for lineup in source_lineups {
        sqlx::query(
            "INSERT INTO auction_match_lineups (
                 guild_id, tournament_id, match_id, team_id, lane, team_member_id, user_id
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(guild_id)
        .bind(tournament_id.as_uuid())
        .bind(final_match_id.as_uuid())
        .bind(winner_team_id)
        .bind(lineup.lane)
        .bind(lineup.team_member_id)
        .bind(lineup.user_id)
        .execute(&mut **transaction)
        .await?;
    }
    let updated = match final_side {
        "LEFT" => {
            sqlx::query(
                "UPDATE auction_matches
             SET left_team_id = $1, revision = revision + 1, updated_by = $2, updated_at = $3
             WHERE guild_id = $4 AND id = $5 AND status = 'LINEUP_PENDING'
               AND lineups_locked_at IS NULL AND left_team_id IS NULL AND deleted_at IS NULL",
            )
            .bind(winner_team_id)
            .bind(actor_id)
            .bind(now)
            .bind(guild_id)
            .bind(final_match_id.as_uuid())
            .execute(&mut **transaction)
            .await?
        }
        "RIGHT" => {
            sqlx::query(
                "UPDATE auction_matches
             SET right_team_id = $1, revision = revision + 1, updated_by = $2, updated_at = $3
             WHERE guild_id = $4 AND id = $5 AND status = 'LINEUP_PENDING'
               AND lineups_locked_at IS NULL AND right_team_id IS NULL AND deleted_at IS NULL",
            )
            .bind(winner_team_id)
            .bind(actor_id)
            .bind(now)
            .bind(guild_id)
            .bind(final_match_id.as_uuid())
            .execute(&mut **transaction)
            .await?
        }
        _ => return Err(AttemptError::InvariantViolation),
    };
    if updated.rows_affected() != 1 {
        return Err(AttemptError::AuctionMatchDownstreamWorkExists);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn reopen_auction_tournament_after_match_revert(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    match_id: AuctionMatchId,
    auction_match: &AuctionMatchLockRow,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let tournament = sqlx::query_as::<_, AuctionTournamentCompletionLockRow>(
        "SELECT format, status
         FROM auction_tournaments
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentNotFound)?;
    let format = parse_auction_format(&tournament.format)?;
    match (format, auction_match.match_key.as_str()) {
        (AuctionFormat::TwentyPerson, "SEMIFINAL_A")
        | (AuctionFormat::TwentyPerson, "SEMIFINAL_B") => {
            let final_side = if auction_match.match_key == "SEMIFINAL_A" {
                "LEFT"
            } else {
                "RIGHT"
            };
            clear_auction_semifinal_final_propagation(
                transaction,
                guild_id,
                tournament_id,
                match_id,
                auction_match,
                final_side,
                actor_id,
                now,
            )
            .await?;
            Ok(())
        }
        (AuctionFormat::TenPerson, "FINAL") | (AuctionFormat::TwentyPerson, "FINAL") => {
            let updated = sqlx::query(
                "UPDATE auction_tournaments
                 SET status = 'IN_GAME',
                     champion_team_id = NULL,
                     ended_at = NULL,
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2
                 WHERE guild_id = $3
                   AND id = $4
                   AND status = 'COMPLETED'
                   AND deleted_at IS NULL",
            )
            .bind(actor_id)
            .bind(now)
            .bind(guild_id)
            .bind(tournament_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(AttemptError::AuctionMatchTournamentNotReady);
            }
            Ok(())
        }
        _ => Err(AttemptError::InvariantViolation),
    }
}

#[allow(clippy::too_many_arguments)]
async fn clear_auction_semifinal_final_propagation(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    semifinal_match_id: AuctionMatchId,
    semifinal: &AuctionMatchLockRow,
    final_side: &str,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let winner_team_id = semifinal
        .winner_team_id
        .ok_or(AttemptError::InvariantViolation)?;
    let (final_match_id, final_match) =
        lock_auction_match_by_key(transaction, guild_id, tournament_id, "FINAL").await?;
    let target_team_id = match final_side {
        "LEFT" => final_match.left_team_id,
        "RIGHT" => final_match.right_team_id,
        _ => return Err(AttemptError::InvariantViolation),
    }
    .ok_or(AttemptError::InvariantViolation)?;
    if target_team_id != winner_team_id {
        return Err(AttemptError::InvariantViolation);
    }
    if auction_final_has_downstream_work(
        transaction,
        guild_id,
        tournament_id,
        final_match_id,
        target_team_id,
        final_match.lineups_locked_at,
        &final_match.status,
    )
    .await?
    {
        return Err(AttemptError::AuctionMatchDownstreamWorkExists);
    }
    let deleted = sqlx::query(
        "DELETE FROM auction_match_lineups
         WHERE guild_id = $1 AND tournament_id = $2 AND match_id = $3 AND team_id = $4",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .bind(final_match_id.as_uuid())
    .bind(target_team_id)
    .execute(&mut **transaction)
    .await?;
    if deleted.rows_affected() != 5 {
        return Err(AttemptError::InvariantViolation);
    }
    let updated = match final_side {
        "LEFT" => {
            sqlx::query(
                "UPDATE auction_matches
             SET left_team_id = NULL,
                 winner_team_id = NULL,
                 ended_at = NULL,
                 revision = revision + 1,
                 updated_by = $1,
                 updated_at = $2
             WHERE guild_id = $3 AND id = $4
               AND status = 'LINEUP_PENDING'
               AND lineups_locked_at IS NULL
               AND left_team_id = $5
               AND deleted_at IS NULL",
            )
            .bind(actor_id)
            .bind(now)
            .bind(guild_id)
            .bind(final_match_id.as_uuid())
            .bind(target_team_id)
            .execute(&mut **transaction)
            .await?
        }
        "RIGHT" => {
            sqlx::query(
                "UPDATE auction_matches
             SET right_team_id = NULL,
                 winner_team_id = NULL,
                 ended_at = NULL,
                 revision = revision + 1,
                 updated_by = $1,
                 updated_at = $2
             WHERE guild_id = $3 AND id = $4
               AND status = 'LINEUP_PENDING'
               AND lineups_locked_at IS NULL
               AND right_team_id = $5
               AND deleted_at IS NULL",
            )
            .bind(actor_id)
            .bind(now)
            .bind(guild_id)
            .bind(final_match_id.as_uuid())
            .bind(target_team_id)
            .execute(&mut **transaction)
            .await?
        }
        _ => return Err(AttemptError::InvariantViolation),
    };
    if updated.rows_affected() != 1 {
        return Err(AttemptError::AuctionMatchDownstreamWorkExists);
    }
    let _ = semifinal_match_id;
    Ok(())
}

async fn mark_auction_tournament_bracket_locked(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE auction_tournaments
         SET bracket_locked_at = $1,
             bracket_locked_by = $2,
             revision = revision + 1,
             updated_by = $2,
             updated_at = $1
         WHERE guild_id = $3
           AND id = $4
           AND status = 'BRACKET_SETUP'
           AND bracket_locked_at IS NULL
           AND deleted_at IS NULL
         RETURNING revision",
    )
    .bind(now)
    .bind(actor_id)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentBracketLocked)
}

async fn ensure_auction_tournament_matches_have_no_result_artifacts(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<(), AttemptError> {
    if auction_tournament_matches_have_result_artifacts(transaction, guild_id, tournament_id)
        .await?
    {
        Err(AttemptError::AuctionTournamentBracketRecoveryUnavailable)
    } else {
        Ok(())
    }
}

async fn auction_tournament_matches_have_result_artifacts(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<bool, AttemptError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM auction_matches AS auction_match
             WHERE auction_match.guild_id = $1
               AND auction_match.tournament_id = $2
               AND auction_match.deleted_at IS NULL
               AND auction_match.status <> 'CANCELLED'
               AND (
                   EXISTS (
                       SELECT 1
                       FROM games
                       WHERE games.guild_id = auction_match.guild_id
                         AND games.auction_match_id = auction_match.id
                   )
                   OR EXISTS (
                       SELECT 1
                       FROM game_inputs
                       WHERE game_inputs.guild_id = auction_match.guild_id
                         AND game_inputs.auction_match_id = auction_match.id
                   )
               )
         )",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn cancel_unstarted_auction_tournament_matches(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let cancelled = sqlx::query(
        "UPDATE auction_matches
         SET status = 'CANCELLED',
             winner_team_id = NULL,
             lineups_locked_at = NULL,
             lineups_locked_by = NULL,
             ended_at = $1,
             revision = revision + 1,
             updated_by = $2,
             updated_at = $1
         WHERE guild_id = $3
           AND tournament_id = $4
           AND status <> 'CANCELLED'
           AND deleted_at IS NULL",
    )
    .bind(now)
    .bind(actor_id)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if cancelled.rows_affected() == 0 {
        return Err(AttemptError::AuctionTournamentBracketRecoveryUnavailable);
    }
    Ok(())
}

async fn cancel_active_auction_tournament_matches(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "UPDATE auction_matches
         SET status = 'CANCELLED',
             winner_team_id = NULL,
             lineups_locked_at = NULL,
             lineups_locked_by = NULL,
             ended_at = $1,
             revision = revision + 1,
             updated_by = $2,
             updated_at = $1
         WHERE guild_id = $3
           AND tournament_id = $4
           AND status <> 'CANCELLED'
           AND deleted_at IS NULL",
    )
    .bind(now)
    .bind(actor_id)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn cancel_auction_tournament_state(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE auction_tournaments
         SET status = 'CANCELLED',
             bracket_locked_at = NULL,
             bracket_locked_by = NULL,
             champion_team_id = NULL,
             ended_at = $1,
             revision = revision + 1,
             updated_by = $2,
             updated_at = $1
         WHERE guild_id = $3
           AND id = $4
           AND status IN ('BIDDING', 'BRACKET_SETUP', 'IN_GAME')
           AND deleted_at IS NULL
         RETURNING revision",
    )
    .bind(now)
    .bind(actor_id)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentCancelUnavailable(
        AuctionTournamentStatus::Cancelled,
    ))
}

async fn cancel_auction_tournament_source_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE auction_recruitments
         SET status = 'CANCELLED',
             revision = revision + 1,
             updated_by = $1,
             updated_at = $2,
             cancelled_at = $2
         WHERE guild_id = $3
           AND id = $4
           AND status = 'CONVERTED'
         RETURNING revision",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentSourceNotReady(
        RecruitmentStatus::Converted,
    ))
}

async fn unlock_auction_tournament_bracket(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE auction_tournaments
         SET status = 'BRACKET_SETUP',
             bracket_locked_at = NULL,
             bracket_locked_by = NULL,
             champion_team_id = NULL,
             ended_at = NULL,
             revision = revision + 1,
             updated_by = $1,
             updated_at = $2
         WHERE guild_id = $3
           AND id = $4
           AND status IN ('BRACKET_SETUP', 'IN_GAME')
           AND bracket_locked_at IS NOT NULL
           AND deleted_at IS NULL
         RETURNING revision",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::AuctionTournamentBracketRecoveryUnavailable)
}

async fn lock_auction_tournament_teams(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<Vec<AuctionTournamentTeamRow>, AttemptError> {
    Ok(sqlx::query_as::<_, AuctionTournamentTeamRow>(
        "SELECT id, team_index, team_name, initial_points, current_points, point_revision
         FROM auction_teams
         WHERE guild_id = $1 AND tournament_id = $2
         ORDER BY team_index
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn lock_auction_tournament_team_members(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<Vec<AuctionTeamMemberLockRow>, AttemptError> {
    Ok(sqlx::query_as::<_, AuctionTeamMemberLockRow>(
        "SELECT id, team_id, user_id, roster_slot, acquired_via
         FROM auction_team_members
         WHERE guild_id = $1 AND tournament_id = $2 AND released_at IS NULL
         ORDER BY team_id, roster_slot, user_id
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn lock_auction_tournament_candidates(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<Vec<AuctionTournamentCandidateLockRow>, AttemptError> {
    Ok(sqlx::query_as::<_, AuctionTournamentCandidateLockRow>(
        "SELECT user_id, declared_lane, mmr_at_start, snapshot_origin, queue_position, state
         FROM auction_bid_candidates
         WHERE guild_id = $1 AND tournament_id = $2
         ORDER BY queue_position, user_id
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn lock_auction_tournament_lots(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<Vec<AuctionLotLockRow>, AttemptError> {
    Ok(sqlx::query_as::<_, AuctionLotLockRow>(
        "SELECT id, draw_sequence, target_user_id, status
         FROM auction_lots
         WHERE guild_id = $1 AND tournament_id = $2
         ORDER BY draw_sequence, id
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn read_auction_tournament_actions(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<Vec<AuctionActionLockRow>, AttemptError> {
    Ok(sqlx::query_as::<_, AuctionActionLockRow>(
        "SELECT id, sequence, kind, target_user_id, team_id, lot_id, points, reverses_action_id
         FROM auction_actions
         WHERE guild_id = $1 AND tournament_id = $2
         ORDER BY sequence, id",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn read_auction_tournament_point_ledger(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<Vec<AuctionPointLedgerLockRow>, AttemptError> {
    Ok(sqlx::query_as::<_, AuctionPointLedgerLockRow>(
        "SELECT id, team_id, point_revision, reason, source_action_id, points_before, points_after
         FROM auction_point_ledger
         WHERE guild_id = $1 AND tournament_id = $2
         ORDER BY team_id, point_revision, id",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn resolve_series_source_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<RecruitmentId, AttemptError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT source_recruitment_id
         FROM series
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .map(RecruitmentId::from_uuid)
    .ok_or(AttemptError::SeriesNotFound)
}

async fn lock_series(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<SeriesLockRow, AttemptError> {
    sqlx::query_as::<_, SeriesLockRow>(
        "SELECT source_recruitment_id, best_of, hard_fearless, status, revision
         FROM series
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::SeriesNotFound)
}

async fn confirmed_series_game_count(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<i64, AttemptError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM games
         WHERE guild_id = $1 AND series_id = $2 AND status = 'CONFIRMED'",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn confirmed_auction_match_game_count(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
) -> Result<i64, AttemptError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM games
         WHERE guild_id = $1 AND auction_match_id = $2 AND status = 'CONFIRMED'",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn confirmed_auction_tournament_game_count(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
) -> Result<i64, AttemptError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM games AS game
         JOIN auction_matches AS auction_match
           ON auction_match.guild_id = game.guild_id
          AND auction_match.id = game.auction_match_id
         WHERE game.guild_id = $1
           AND auction_match.tournament_id = $2
           AND auction_match.deleted_at IS NULL
           AND auction_match.status <> 'CANCELLED'
           AND game.status = 'CONFIRMED'",
    )
    .bind(guild_id)
    .bind(tournament_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn confirmed_auction_match_score(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
) -> Result<(i64, i64), AttemptError> {
    let row = sqlx::query_as::<_, AuctionMatchScoreRow>(
        "SELECT count(id) FILTER (
                    WHERE (side_preset = 'DEFAULT' AND winning_side = 'BLUE')
                       OR (side_preset = 'SWAPPED' AND winning_side = 'RED')
                ) AS left_score,
                count(id) FILTER (
                    WHERE (side_preset = 'DEFAULT' AND winning_side = 'RED')
                       OR (side_preset = 'SWAPPED' AND winning_side = 'BLUE')
                ) AS right_score
         FROM games
         WHERE guild_id = $1 AND auction_match_id = $2 AND status = 'CONFIRMED'",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?;
    Ok((row.left_score, row.right_score))
}

async fn locked_series_roster(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<Vec<SeriesRosterRow>, AttemptError> {
    Ok(sqlx::query_as::<_, SeriesRosterRow>(
        "SELECT user_id, team, lane
         FROM series_participants
         WHERE guild_id = $1 AND series_id = $2
         ORDER BY user_id, lane
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

fn ensure_complete_series_roster_rows(roster: &[SeriesRosterRow]) -> Result<(), AttemptError> {
    if roster.len() != 10 {
        return Err(AttemptError::InvariantViolation);
    }
    let users = roster
        .iter()
        .map(|row| row.user_id.clone())
        .collect::<HashSet<_>>();
    if users.len() != roster.len() {
        return Err(AttemptError::InvariantViolation);
    }
    for team in ["TEAM_1", "TEAM_2"] {
        let lanes = roster
            .iter()
            .filter(|row| row.team == team)
            .map(|row| row.lane)
            .collect::<HashSet<_>>();
        if lanes.len() != 5
            || !lanes.contains(&Lane::Top)
            || !lanes.contains(&Lane::Jungle)
            || !lanes.contains(&Lane::Mid)
            || !lanes.contains(&Lane::Bottom)
            || !lanes.contains(&Lane::Support)
        {
            return Err(AttemptError::InvariantViolation);
        }
    }
    if roster
        .iter()
        .any(|row| !matches!(row.team.as_str(), "TEAM_1" | "TEAM_2"))
    {
        return Err(AttemptError::InvariantViolation);
    }
    Ok(())
}

fn next_series_set_number(confirmed_game_count: i64, best_of: i16) -> Result<i16, AttemptError> {
    let next =
        i16::try_from(confirmed_game_count + 1).map_err(|_| AttemptError::InvariantViolation)?;
    if next < 1 || next > best_of {
        return Err(AttemptError::SeriesNoCurrentSet);
    }
    Ok(next)
}

fn can_early_complete_series(best_of: i16, confirmed_game_count: i64) -> bool {
    matches!(best_of, 3 | 5) && confirmed_game_count > 0
}

async fn ensure_locked_series_draft_input(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
    set_number: i16,
    input_id: Uuid,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<Uuid, AttemptError> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM game_inputs
         WHERE guild_id = $1
           AND series_id = $2
           AND set_number = $3
           AND status = 'DRAFT'
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .bind(set_number)
    .fetch_optional(&mut **transaction)
    .await?
    {
        return Ok(id);
    }
    sqlx::query(
        "INSERT INTO game_inputs (
             guild_id, id, series_id, auction_match_id, set_number, side_preset, side_selected,
             winning_side, patch_version, status, updated_by, updated_at
         ) VALUES ($1, $2, $3, NULL, $4, 'DEFAULT', false, NULL, NULL, 'DRAFT', $5, $6)",
    )
    .bind(guild_id)
    .bind(input_id)
    .bind(series_id.as_uuid())
    .bind(set_number)
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(input_id)
}

async fn ensure_locked_auction_match_draft_input(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    set_number: i16,
    input_id: Uuid,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<Uuid, AttemptError> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM game_inputs
         WHERE guild_id = $1
           AND auction_match_id = $2
           AND set_number = $3
           AND status = 'DRAFT'
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .bind(set_number)
    .fetch_optional(&mut **transaction)
    .await?
    {
        return Ok(id);
    }
    sqlx::query(
        "INSERT INTO game_inputs (
             guild_id, id, series_id, auction_match_id, set_number, side_preset, side_selected,
             winning_side, patch_version, status, updated_by, updated_at
         ) VALUES ($1, $2, NULL, $3, $4, 'DEFAULT', false, NULL, NULL, 'DRAFT', $5, $6)",
    )
    .bind(guild_id)
    .bind(input_id)
    .bind(match_id.as_uuid())
    .bind(set_number)
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(input_id)
}

fn auction_match_roster_from_lineups(
    lineups: &[AuctionMatchLineupRow],
    left_team_id: Uuid,
    right_team_id: Uuid,
) -> Result<Vec<SeriesRosterRow>, AttemptError> {
    let roster = lineups
        .iter()
        .map(|lineup| {
            let team = if lineup.team_id == left_team_id {
                "TEAM_1"
            } else if lineup.team_id == right_team_id {
                "TEAM_2"
            } else {
                return Err(AttemptError::InvariantViolation);
            };
            Ok(SeriesRosterRow {
                user_id: lineup.user_id.clone(),
                team: team.to_owned(),
                lane: lineup.lane,
            })
        })
        .collect::<Result<Vec<_>, AttemptError>>()?;
    ensure_complete_series_roster_rows(&roster)?;
    Ok(roster)
}

async fn apply_series_pick_ban_operation(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    input_id: Uuid,
    roster: &[SeriesRosterRow],
    operation: &SeriesPickBanOperation,
) -> Result<(), AttemptError> {
    if operation_requires_selected_side(operation) {
        ensure_game_input_side_selected(transaction, guild_id, input_id).await?;
    }

    match operation {
        SeriesPickBanOperation::SetSide { side_preset } => {
            canonical_side_preset(side_preset)?;
            let updated = sqlx::query(
                "UPDATE game_inputs SET side_preset = $1, side_selected = true
                 WHERE guild_id = $2 AND id = $3 AND status = 'DRAFT'",
            )
            .bind(side_preset)
            .bind(guild_id)
            .bind(input_id)
            .execute(&mut **transaction)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
        }
        SeriesPickBanOperation::SetPick {
            player_id,
            champion_id,
        } => {
            ensure_roster_player(roster, player_id)?;
            validate_champion_id(champion_id)?;
            sqlx::query(
                "INSERT INTO game_input_picks (guild_id, game_input_id, user_id, champion_id)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (guild_id, game_input_id, user_id)
                 DO UPDATE SET champion_id = EXCLUDED.champion_id",
            )
            .bind(guild_id)
            .bind(input_id)
            .bind(player_id)
            .bind(champion_id)
            .execute(&mut **transaction)
            .await?;
        }
        SeriesPickBanOperation::ClearPick { player_id } => {
            ensure_roster_player(roster, player_id)?;
            sqlx::query(
                "DELETE FROM game_input_picks
                 WHERE guild_id = $1 AND game_input_id = $2 AND user_id = $3",
            )
            .bind(guild_id)
            .bind(input_id)
            .bind(player_id)
            .execute(&mut **transaction)
            .await?;
        }
        SeriesPickBanOperation::SetBan {
            side,
            slot,
            champion_id,
        } => {
            canonical_blue_red_side(side)?;
            ensure_ban_slot(*slot)?;
            validate_champion_id(champion_id)?;
            sqlx::query(
                "INSERT INTO game_input_bans (guild_id, game_input_id, side, slot, champion_id)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (guild_id, game_input_id, side, slot)
                 DO UPDATE SET champion_id = EXCLUDED.champion_id",
            )
            .bind(guild_id)
            .bind(input_id)
            .bind(side)
            .bind(slot)
            .bind(champion_id)
            .execute(&mut **transaction)
            .await?;
        }
        SeriesPickBanOperation::ClearBan { side, slot } => {
            canonical_blue_red_side(side)?;
            ensure_ban_slot(*slot)?;
            sqlx::query(
                "DELETE FROM game_input_bans
                 WHERE guild_id = $1 AND game_input_id = $2 AND side = $3 AND slot = $4",
            )
            .bind(guild_id)
            .bind(input_id)
            .bind(side)
            .bind(slot)
            .execute(&mut **transaction)
            .await?;
        }
        SeriesPickBanOperation::ApplyBulk { items } => {
            for item in items {
                apply_series_pick_ban_bulk_item(transaction, guild_id, input_id, roster, item)
                    .await?;
            }
        }
    }
    Ok(())
}

fn operation_requires_selected_side(operation: &SeriesPickBanOperation) -> bool {
    !matches!(operation, SeriesPickBanOperation::SetSide { .. })
}

async fn ensure_game_input_side_selected(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    input_id: Uuid,
) -> Result<(), AttemptError> {
    let side_selected = sqlx::query_scalar::<_, bool>(
        "SELECT side_selected
         FROM game_inputs
         WHERE guild_id = $1 AND id = $2 AND status = 'DRAFT'
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(input_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)?;
    side_selected
        .then_some(())
        .ok_or(AttemptError::SeriesGameSideRequired)
}

async fn apply_series_pick_ban_bulk_item(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    input_id: Uuid,
    roster: &[SeriesRosterRow],
    item: &SeriesPickBanBulkItem,
) -> Result<(), AttemptError> {
    match item {
        SeriesPickBanBulkItem::SetPick {
            player_id,
            champion_id,
        } => {
            ensure_roster_player(roster, player_id)?;
            validate_champion_id(champion_id)?;
            sqlx::query(
                "INSERT INTO game_input_picks (guild_id, game_input_id, user_id, champion_id)
				 VALUES ($1, $2, $3, $4)
				 ON CONFLICT (guild_id, game_input_id, user_id)
				 DO UPDATE SET champion_id = EXCLUDED.champion_id",
            )
            .bind(guild_id)
            .bind(input_id)
            .bind(player_id)
            .bind(champion_id)
            .execute(&mut **transaction)
            .await?;
        }
        SeriesPickBanBulkItem::SetBan {
            side,
            slot,
            champion_id,
        } => {
            canonical_blue_red_side(side)?;
            ensure_ban_slot(*slot)?;
            validate_champion_id(champion_id)?;
            sqlx::query(
                "INSERT INTO game_input_bans (guild_id, game_input_id, side, slot, champion_id)
				 VALUES ($1, $2, $3, $4, $5)
				 ON CONFLICT (guild_id, game_input_id, side, slot)
				 DO UPDATE SET champion_id = EXCLUDED.champion_id",
            )
            .bind(guild_id)
            .bind(input_id)
            .bind(side)
            .bind(slot)
            .bind(champion_id)
            .execute(&mut **transaction)
            .await?;
        }
    }
    Ok(())
}

async fn replace_series_draft_from_submit(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    input_id: Uuid,
    roster: &[SeriesRosterRow],
    command: &SeriesGameSubmitCommand,
) -> Result<(), AttemptError> {
    canonical_side_preset(&command.side_preset)?;
    canonical_blue_red_side(&command.winner)?;
    validate_patch_version(command.patch_version.as_deref())?;
    let stored_side = sqlx::query_as::<_, (String, bool)>(
        "SELECT side_preset, side_selected
         FROM game_inputs
         WHERE guild_id = $1 AND id = $2 AND status = 'DRAFT'
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(input_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)?;
    if stored_side.1 && stored_side.0 != command.side_preset {
        return Err(AttemptError::SeriesGameSideMismatch {
            expected: stored_side.0,
            actual: command.side_preset.clone(),
        });
    }
    if let Some(picks) = command.picks.as_ref() {
        validate_submit_picks(picks, roster)?;
        sqlx::query("DELETE FROM game_input_picks WHERE guild_id = $1 AND game_input_id = $2")
            .bind(guild_id)
            .bind(input_id)
            .execute(&mut **transaction)
            .await?;
        for (user_id, champion_id) in picks {
            if let Some(champion_id) = champion_id {
                sqlx::query(
                    "INSERT INTO game_input_picks (guild_id, game_input_id, user_id, champion_id)
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(guild_id)
                .bind(input_id)
                .bind(user_id)
                .bind(champion_id)
                .execute(&mut **transaction)
                .await?;
            }
        }
    }
    if let Some(bans) = command.bans.as_ref() {
        validate_submit_bans(bans)?;
        sqlx::query("DELETE FROM game_input_bans WHERE guild_id = $1 AND game_input_id = $2")
            .bind(guild_id)
            .bind(input_id)
            .execute(&mut **transaction)
            .await?;
        insert_series_input_bans(transaction, guild_id, input_id, bans).await?;
    }
    let updated = sqlx::query(
        "UPDATE game_inputs
         SET side_preset = $1, side_selected = true, winning_side = $2, patch_version = $3
         WHERE guild_id = $4 AND id = $5 AND status = 'DRAFT'",
    )
    .bind(&command.side_preset)
    .bind(&command.winner)
    .bind(&command.patch_version)
    .bind(guild_id)
    .bind(input_id)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }
    Ok(())
}

async fn insert_series_input_bans(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    input_id: Uuid,
    bans: &SeriesPickBanBansSnapshot,
) -> Result<(), AttemptError> {
    for (side, slots) in [("BLUE", &bans.blue), ("RED", &bans.red)] {
        for (index, champion_id) in slots.iter().enumerate() {
            if let Some(champion_id) = champion_id {
                let slot =
                    i16::try_from(index + 1).map_err(|_| AttemptError::InvariantViolation)?;
                sqlx::query(
                    "INSERT INTO game_input_bans (guild_id, game_input_id, side, slot, champion_id)
                     VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(guild_id)
                .bind(input_id)
                .bind(side)
                .bind(slot)
                .bind(champion_id)
                .execute(&mut **transaction)
                .await?;
            }
        }
    }
    Ok(())
}

async fn validate_series_pick_ban_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
    hard_fearless: bool,
) -> Result<(), AttemptError> {
    let picks = sqlx::query_scalar::<_, String>(
        "SELECT pick.champion_id
		 FROM game_input_picks AS pick
		 JOIN game_inputs AS input
		   ON input.guild_id = pick.guild_id
		  AND input.id = pick.game_input_id
		 WHERE input.guild_id = $1
		   AND input.series_id = $2
		   AND input.status = 'DRAFT'
		 ORDER BY pick.champion_id, pick.user_id",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let bans = sqlx::query_scalar::<_, String>(
        "SELECT ban.champion_id
		 FROM game_input_bans AS ban
		 JOIN game_inputs AS input
		   ON input.guild_id = ban.guild_id
		  AND input.id = ban.game_input_id
		 WHERE input.guild_id = $1
		   AND input.series_id = $2
		   AND input.status = 'DRAFT'
		 ORDER BY ban.champion_id, ban.side, ban.slot",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;

    let mut seen = HashSet::with_capacity(picks.len() + bans.len());
    for champion_id in picks.iter().chain(&bans) {
        if !seen.insert(champion_id.as_str()) {
            return Err(AttemptError::SeriesGameChampionDuplicate {
                champion_id: champion_id.clone(),
            });
        }
    }
    if !hard_fearless || picks.is_empty() {
        return Ok(());
    }

    let used = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT participant.champion_id
		 FROM game_participants AS participant
		 JOIN games AS game
		   ON game.guild_id = participant.guild_id
		  AND game.id = participant.game_id
		 WHERE game.guild_id = $1
		   AND game.series_id = $2
		   AND game.status = 'CONFIRMED'
		   AND participant.champion_id IS NOT NULL",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .collect::<HashSet<_>>();
    for champion_id in picks {
        if used.contains(&champion_id) {
            return Err(AttemptError::SeriesHardFearlessChampionUsed { champion_id });
        }
    }
    Ok(())
}

async fn validate_auction_match_pick_ban_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    hard_fearless: bool,
) -> Result<(), AttemptError> {
    let picks = sqlx::query_scalar::<_, String>(
        "SELECT pick.champion_id
         FROM game_input_picks AS pick
         JOIN game_inputs AS input
           ON input.guild_id = pick.guild_id
          AND input.id = pick.game_input_id
         WHERE input.guild_id = $1
           AND input.auction_match_id = $2
           AND input.status = 'DRAFT'
         ORDER BY pick.champion_id, pick.user_id",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let bans = sqlx::query_scalar::<_, String>(
        "SELECT ban.champion_id
         FROM game_input_bans AS ban
         JOIN game_inputs AS input
           ON input.guild_id = ban.guild_id
          AND input.id = ban.game_input_id
         WHERE input.guild_id = $1
           AND input.auction_match_id = $2
           AND input.status = 'DRAFT'
         ORDER BY ban.champion_id, ban.side, ban.slot",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;

    let mut seen = HashSet::with_capacity(picks.len() + bans.len());
    for champion_id in picks.iter().chain(&bans) {
        if !seen.insert(champion_id.as_str()) {
            return Err(AttemptError::AuctionMatchGameChampionDuplicate {
                champion_id: champion_id.clone(),
            });
        }
    }
    if !hard_fearless || picks.is_empty() {
        return Ok(());
    }
    let used = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT participant.champion_id
         FROM game_participants AS participant
         JOIN games AS game
           ON game.guild_id = participant.guild_id
          AND game.id = participant.game_id
         WHERE game.guild_id = $1
           AND game.auction_match_id = $2
           AND game.status = 'CONFIRMED'
           AND participant.champion_id IS NOT NULL",
    )
    .bind(guild_id)
    .bind(match_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .collect::<HashSet<_>>();
    for champion_id in picks {
        if used.contains(&champion_id) {
            return Err(AttemptError::AuctionMatchHardFearlessChampionUsed { champion_id });
        }
    }
    Ok(())
}

async fn series_input_picks(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    input_id: Uuid,
    roster: &[SeriesRosterRow],
) -> Result<BTreeMap<String, Option<String>>, AttemptError> {
    let player_ids = roster
        .iter()
        .map(|player| player.user_id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let input = SeriesPickBanGameInputRow {
        id: input_id,
        set_number: 1,
        side_preset: "DEFAULT".to_owned(),
        side_selected: true,
        winning_side: None,
        patch_version: None,
    };
    Ok(
        series_pick_ban_game_snapshot(transaction, guild_id, &input, &player_ids)
            .await?
            .picks,
    )
}

async fn series_input_bans(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    input_id: Uuid,
) -> Result<SeriesPickBanBansSnapshot, AttemptError> {
    let rows = sqlx::query_as::<_, SeriesPickBanBanRow>(
        "SELECT side, slot, champion_id
         FROM game_input_bans
         WHERE guild_id = $1 AND game_input_id = $2",
    )
    .bind(guild_id)
    .bind(input_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut bans = SeriesPickBanBansSnapshot {
        blue: vec![None; 5],
        red: vec![None; 5],
    };
    for row in rows {
        let index = usize::try_from(
            row.slot
                .checked_sub(1)
                .ok_or(AttemptError::InvariantViolation)?,
        )
        .map_err(|_| AttemptError::InvariantViolation)?;
        let slot = match row.side.as_str() {
            "BLUE" => bans.blue.get_mut(index),
            "RED" => bans.red.get_mut(index),
            _ => return Err(AttemptError::InvariantViolation),
        }
        .ok_or(AttemptError::InvariantViolation)?;
        if slot.replace(row.champion_id).is_some() {
            return Err(AttemptError::InvariantViolation);
        }
    }
    Ok(bans)
}

async fn insert_series_game_participants(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    game_id: GameId,
    roster: &[SeriesRosterRow],
    picks: &BTreeMap<String, Option<String>>,
) -> Result<(), AttemptError> {
    for player in roster {
        let champion_id = picks
            .get(player.user_id.as_str())
            .ok_or(AttemptError::InvariantViolation)?;
        sqlx::query(
            "INSERT INTO game_participants (guild_id, game_id, user_id, team, lane, champion_id)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(guild_id)
        .bind(game_id.as_uuid())
        .bind(&player.user_id)
        .bind(&player.team)
        .bind(player.lane)
        .bind(champion_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn insert_series_game_bans(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    game_id: GameId,
    bans: &SeriesPickBanBansSnapshot,
) -> Result<(), AttemptError> {
    for (side, slots) in [("BLUE", &bans.blue), ("RED", &bans.red)] {
        for (index, champion_id) in slots.iter().enumerate() {
            if let Some(champion_id) = champion_id {
                let slot =
                    i16::try_from(index + 1).map_err(|_| AttemptError::InvariantViolation)?;
                sqlx::query(
                    "INSERT INTO game_bans (guild_id, game_id, side, slot, champion_id)
                     VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(guild_id)
                .bind(game_id.as_uuid())
                .bind(side)
                .bind(slot)
                .bind(champion_id)
                .execute(&mut **transaction)
                .await?;
            }
        }
    }
    Ok(())
}

async fn apply_series_game_mmr(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    season_id: SeasonId,
    game_id: GameId,
    roster: &[SeriesRosterRow],
    winner: RosterTeam,
    actor_id: &UserId,
    request_id: &str,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let locked_rows = mmr::ensure_and_lock_mmr_rows(
        transaction,
        guild_id,
        season_id,
        roster.iter().map(|player| MmrLaneKey {
            user_id: player.user_id.clone(),
            lane: player.lane,
        }),
        now,
    )
    .await?;
    let mmr_by_key = locked_rows
        .into_iter()
        .map(|row| ((row.key.user_id.clone(), row.key.lane), row))
        .collect::<HashMap<_, _>>();
    if mmr_by_key.len() != roster.len() {
        return Err(AttemptError::InvariantViolation);
    }

    let mut matchups = Vec::with_capacity(5);
    for lane in [
        Lane::Top,
        Lane::Jungle,
        Lane::Mid,
        Lane::Bottom,
        Lane::Support,
    ] {
        let team_1 = roster
            .iter()
            .find(|player| player.team == "TEAM_1" && player.lane == lane)
            .ok_or(AttemptError::InvariantViolation)?;
        let team_2 = roster
            .iter()
            .find(|player| player.team == "TEAM_2" && player.lane == lane)
            .ok_or(AttemptError::InvariantViolation)?;
        let team_1_mmr = mmr_by_key
            .get(&(team_1.user_id.clone(), lane))
            .ok_or(AttemptError::InvariantViolation)?
            .mmr;
        let team_2_mmr = mmr_by_key
            .get(&(team_2.user_id.clone(), lane))
            .ok_or(AttemptError::InvariantViolation)?
            .mmr;
        matchups.push(LaneMatchup {
            lane,
            team_1: MatchupPlayer {
                user_id: team_1.user_id.clone(),
                mmr: team_1_mmr,
            },
            team_2: MatchupPlayer {
                user_id: team_2.user_id.clone(),
                mmr: team_2_mmr,
            },
        });
    }

    for result in apply_game_elo(matchups, winner) {
        for (update, won) in [
            (result.team_1, winner == RosterTeam::Team1),
            (result.team_2, winner == RosterTeam::Team2),
        ] {
            let locked = mmr_by_key
                .get(&(update.user_id.clone(), result.lane))
                .ok_or(AttemptError::InvariantViolation)?;
            if locked.mmr != update.mmr_before {
                return Err(AttemptError::InvariantViolation);
            }
            let lane_revision = locked
                .revision
                .checked_add(1)
                .ok_or(AttemptError::InvariantViolation)?;
            let games_played = locked
                .games_played
                .checked_add(1)
                .ok_or(AttemptError::InvariantViolation)?;
            let wins = locked
                .wins
                .checked_add(i32::from(won))
                .ok_or(AttemptError::InvariantViolation)?;
            sqlx::query(
                "INSERT INTO mmr_ledger_entries (
                     guild_id, id, season_id, user_id, lane, lane_revision, source_type,
                     source_id, game_id, opponent_user_id, mmr_before, mmr_after, delta,
                     games_played_delta, wins_delta, reverses_entry_id, reversed_source_type,
                     actor_type, actor_id, note, request_id, created_at
                 ) VALUES (
                     $1, $2, $3, $4, $5, $6, 'GAME_RESULT',
                     $7, $7, $8, $9, $10, $11,
                     1, $12, NULL, NULL, 'USER', $13, NULL, $14, $15
                 )",
            )
            .bind(guild_id)
            .bind(Uuid::now_v7())
            .bind(season_id.as_uuid())
            .bind(&update.user_id)
            .bind(result.lane)
            .bind(lane_revision)
            .bind(game_id.as_uuid())
            .bind(&update.opponent_user_id)
            .bind(update.mmr_before)
            .bind(update.mmr_after)
            .bind(update.delta)
            .bind(if won { 1_i16 } else { 0_i16 })
            .bind(actor_id)
            .bind(request_id)
            .bind(now)
            .execute(&mut **transaction)
            .await?;
            let updated = sqlx::query(
                "UPDATE user_lane_mmr
                 SET mmr = $1, games_played = $2, wins = $3, revision = $4, updated_at = $5
                 WHERE guild_id = $6 AND season_id = $7 AND user_id = $8 AND lane = $9",
            )
            .bind(update.mmr_after)
            .bind(games_played)
            .bind(wins)
            .bind(lane_revision)
            .bind(now)
            .bind(guild_id)
            .bind(season_id.as_uuid())
            .bind(&update.user_id)
            .bind(result.lane)
            .execute(&mut **transaction)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
        }
    }
    Ok(())
}

async fn confirmed_series_score(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<(i64, i64), AttemptError> {
    Ok(sqlx::query_as::<_, (i64, i64)>(
        "SELECT
             count(*) FILTER (
                 WHERE (side_preset = 'DEFAULT' AND winning_side = 'BLUE')
                    OR (side_preset = 'SWAPPED' AND winning_side = 'RED')
             ) AS team_1_score,
             count(*) FILTER (
                 WHERE (side_preset = 'DEFAULT' AND winning_side = 'RED')
                    OR (side_preset = 'SWAPPED' AND winning_side = 'BLUE')
             ) AS team_2_score
         FROM games
         WHERE guild_id = $1 AND series_id = $2 AND status = 'CONFIRMED'",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn series_season_id(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<SeasonId, AttemptError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT season_id FROM series WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .map(SeasonId::from_uuid)
    .ok_or(AttemptError::SeriesNotFound)
}

async fn increment_series_revision(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    now: OffsetDateTime,
    status: SeriesStatus,
    winning_team: Option<&str>,
    terminal_reason: Option<&str>,
    ended_at: Option<OffsetDateTime>,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE series
         SET status = $1,
             winning_team = $2,
             terminal_reason = $3,
             ended_at = $4,
             revision = revision + 1,
             updated_by = $5,
             updated_at = $6
         WHERE guild_id = $7 AND id = $8
         RETURNING revision",
    )
    .bind(status)
    .bind(winning_team)
    .bind(terminal_reason)
    .bind(ended_at)
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)
}

fn winner_roster_team(side_preset: &str, winner: &str) -> Result<RosterTeam, AttemptError> {
    match (side_preset, winner) {
        ("DEFAULT", "BLUE") | ("SWAPPED", "RED") => Ok(RosterTeam::Team1),
        ("DEFAULT", "RED") | ("SWAPPED", "BLUE") => Ok(RosterTeam::Team2),
        _ => Err(AttemptError::InvalidGameResult),
    }
}

async fn void_last_confirmed_game(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    parent: ConfirmedGameParent,
    actor_id: &UserId,
    request_id: &str,
    now: OffsetDateTime,
) -> Result<VoidedSeriesGame, AttemptError> {
    let game = match parent {
        ConfirmedGameParent::Series(series_id) => sqlx::query_as::<_, ConfirmedSeriesGameRow>(
            "SELECT id, season_id, source_input_id, game_number
             FROM games
             WHERE guild_id = $1 AND series_id = $2 AND status = 'CONFIRMED'
             ORDER BY game_number DESC
             LIMIT 1
             FOR UPDATE",
        )
        .bind(guild_id)
        .bind(series_id.as_uuid())
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(AttemptError::SeriesNoConfirmedGames)?,
        ConfirmedGameParent::AuctionMatch(match_id) => sqlx::query_as::<_, ConfirmedSeriesGameRow>(
            "SELECT id, season_id, source_input_id, game_number
                 FROM games
                 WHERE guild_id = $1 AND auction_match_id = $2 AND status = 'CONFIRMED'
                 ORDER BY game_number DESC
                 LIMIT 1
                 FOR UPDATE",
        )
        .bind(guild_id)
        .bind(match_id.as_uuid())
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(AttemptError::AuctionMatchNoConfirmedGames)?,
        ConfirmedGameParent::AuctionTournament(tournament_id) => {
            sqlx::query_as::<_, ConfirmedSeriesGameRow>(
                "SELECT game.id, game.season_id, game.source_input_id, game.game_number
                 FROM games AS game
                 JOIN auction_matches AS auction_match
                   ON auction_match.guild_id = game.guild_id
                  AND auction_match.id = game.auction_match_id
                 WHERE game.guild_id = $1
                   AND auction_match.tournament_id = $2
                   AND auction_match.deleted_at IS NULL
                   AND auction_match.status <> 'CANCELLED'
                   AND game.status = 'CONFIRMED'
                 ORDER BY game.confirmed_at DESC, game.id DESC
                 LIMIT 1
                 FOR UPDATE OF game",
            )
            .bind(guild_id)
            .bind(tournament_id.as_uuid())
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(AttemptError::InvariantViolation)?
        }
    };
    let (reversal_actor_type, void_reason) = parent.reversal_metadata();
    // Ledger rows are append-only. The locked game and MMR rows serialize this reversal.
    let ledger_rows = sqlx::query_as::<_, GameResultLedgerRow>(
        "SELECT id, user_id, lane, lane_revision, opponent_user_id, mmr_before, mmr_after,
                delta, games_played_delta, wins_delta
         FROM mmr_ledger_entries
         WHERE guild_id = $1 AND game_id = $2 AND source_type = 'GAME_RESULT'
         ORDER BY user_id, lane",
    )
    .bind(guild_id)
    .bind(game.id)
    .fetch_all(&mut **transaction)
    .await?;
    if ledger_rows.len() != 10 {
        return Err(AttemptError::InvariantViolation);
    }
    let locked_rows = mmr::ensure_and_lock_mmr_rows(
        transaction,
        guild_id,
        SeasonId::from_uuid(game.season_id),
        ledger_rows.iter().map(|row| MmrLaneKey {
            user_id: row.user_id.clone(),
            lane: row.lane,
        }),
        now,
    )
    .await?;
    let current_by_key = locked_rows
        .into_iter()
        .map(|row| ((row.key.user_id.clone(), row.key.lane), row))
        .collect::<HashMap<_, _>>();
    for entry in &ledger_rows {
        let current = current_by_key
            .get(&(entry.user_id.clone(), entry.lane))
            .ok_or(AttemptError::InvariantViolation)?;
        let has_blocking_later_change = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                 SELECT 1
                 FROM mmr_ledger_entries AS later
                 WHERE later.guild_id = $1
                   AND later.season_id = $2
                   AND later.user_id = $3
                   AND later.lane = $4
                   AND later.lane_revision > $5
                   AND (
                       later.source_type NOT IN ('GAME_RESULT', 'GAME_REVERSAL')
                       OR (
                           later.source_type = 'GAME_RESULT'
                           AND NOT EXISTS (
                               SELECT 1
                               FROM mmr_ledger_entries AS reversal
                               WHERE reversal.guild_id = later.guild_id
                                 AND reversal.reverses_entry_id = later.id
                                 AND reversal.source_type = 'GAME_REVERSAL'
                           )
                       )
                   )
             )",
        )
        .bind(guild_id)
        .bind(game.season_id)
        .bind(&entry.user_id)
        .bind(entry.lane)
        .bind(entry.lane_revision)
        .fetch_one(&mut **transaction)
        .await?;
        if current.mmr != entry.mmr_after
            || current.games_played < i32::from(entry.games_played_delta)
            || current.wins < i32::from(entry.wins_delta)
            || has_blocking_later_change
        {
            return Err(parent.mmr_history_error());
        }
    }
    for entry in ledger_rows {
        let current = current_by_key
            .get(&(entry.user_id.clone(), entry.lane))
            .ok_or(AttemptError::InvariantViolation)?;
        let next_revision = current
            .revision
            .checked_add(1)
            .ok_or(AttemptError::InvariantViolation)?;
        let games_played = current
            .games_played
            .checked_sub(i32::from(entry.games_played_delta))
            .ok_or(AttemptError::InvariantViolation)?;
        let wins = current
            .wins
            .checked_sub(i32::from(entry.wins_delta))
            .ok_or(AttemptError::InvariantViolation)?;
        sqlx::query(
            "INSERT INTO mmr_ledger_entries (
                 guild_id, id, season_id, user_id, lane, lane_revision, source_type,
                 source_id, game_id, opponent_user_id, mmr_before, mmr_after, delta,
                 games_played_delta, wins_delta, reverses_entry_id, reversed_source_type,
                 actor_type, actor_id, note, request_id, created_at
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, 'GAME_REVERSAL',
                 $7, $7, $8, $9, $10, $11,
                 $12, $13, $14, 'GAME_RESULT', $15, $16, NULL, $17, $18
             )",
        )
        .bind(guild_id)
        .bind(Uuid::now_v7())
        .bind(game.season_id)
        .bind(&entry.user_id)
        .bind(entry.lane)
        .bind(next_revision)
        .bind(game.id)
        .bind(&entry.opponent_user_id)
        .bind(current.mmr)
        .bind(entry.mmr_before)
        .bind(-entry.delta)
        .bind(-entry.games_played_delta)
        .bind(-entry.wins_delta)
        .bind(entry.id)
        .bind(reversal_actor_type)
        .bind(actor_id)
        .bind(request_id)
        .bind(now)
        .execute(&mut **transaction)
        .await?;
        let updated = sqlx::query(
            "UPDATE user_lane_mmr
             SET mmr = $1, games_played = $2, wins = $3, revision = $4, updated_at = $5
             WHERE guild_id = $6 AND season_id = $7 AND user_id = $8 AND lane = $9",
        )
        .bind(entry.mmr_before)
        .bind(games_played)
        .bind(wins)
        .bind(next_revision)
        .bind(now)
        .bind(guild_id)
        .bind(game.season_id)
        .bind(&entry.user_id)
        .bind(entry.lane)
        .execute(&mut **transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(AttemptError::InvariantViolation);
        }
    }
    let voided = sqlx::query(
        "UPDATE games
         SET status = 'VOIDED', mmr_status = 'REVERSED', voided_by = $1,
             voided_by_type = $2, voided_at = $3, void_reason = $4
         WHERE guild_id = $5 AND id = $6 AND status = 'CONFIRMED'",
    )
    .bind(actor_id)
    .bind(reversal_actor_type)
    .bind(now)
    .bind(void_reason)
    .bind(guild_id)
    .bind(game.id)
    .execute(&mut **transaction)
    .await?;
    if voided.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }
    let superseded = sqlx::query(
        "UPDATE game_inputs
         SET status = 'SUPERSEDED', updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND id = $4 AND status = 'SUBMITTED'",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(game.source_input_id)
    .execute(&mut **transaction)
    .await?;
    if superseded.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }
    Ok(VoidedSeriesGame {
        game_id: GameId::from_uuid(game.id),
        set_number: game.game_number,
    })
}

async fn void_last_confirmed_series_game(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    request_id: &str,
    now: OffsetDateTime,
) -> Result<VoidedSeriesGame, AttemptError> {
    void_last_confirmed_game(
        transaction,
        guild_id,
        ConfirmedGameParent::Series(series_id),
        actor_id,
        request_id,
        now,
    )
    .await
}

async fn void_last_confirmed_auction_match_game(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    match_id: AuctionMatchId,
    actor_id: &UserId,
    request_id: &str,
    now: OffsetDateTime,
) -> Result<VoidedSeriesGame, AttemptError> {
    void_last_confirmed_game(
        transaction,
        guild_id,
        ConfirmedGameParent::AuctionMatch(match_id),
        actor_id,
        request_id,
        now,
    )
    .await
}

async fn void_last_confirmed_auction_tournament_game(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    tournament_id: AuctionTournamentId,
    actor_id: &UserId,
    request_id: &str,
    now: OffsetDateTime,
) -> Result<VoidedSeriesGame, AttemptError> {
    void_last_confirmed_game(
        transaction,
        guild_id,
        ConfirmedGameParent::AuctionTournament(tournament_id),
        actor_id,
        request_id,
        now,
    )
    .await
}

async fn lock_entry_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<(), AttemptError> {
    let _ = lock_entry_draft_revision(transaction, guild_id, recruitment_id).await?;
    Ok(())
}

async fn lock_entry_draft_revision(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "SELECT revision
         FROM recruitment_entry_drafts
         WHERE guild_id = $1 AND recruitment_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingEntryDraft)
}

async fn close_recruitment_for_entry(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(RecruitmentSnapshot, RecruitmentSnapshot), AttemptError> {
    let locked = lock_recruitment(transaction, guild_id, recruitment_id).await?;
    ensure_open(locked.status)?;
    let before = recruitment_snapshot(transaction, guild_id, recruitment_id).await?;
    let joined_count = joined_participant_count(transaction, guild_id, recruitment_id).await?;
    if joined_count != i64::from(locked.target_count) {
        return Err(AttemptError::CloseRequiresFullRoster {
            joined: joined_count,
            target: locked.target_count,
        });
    }

    let closed = sqlx::query(
        "UPDATE recruitments
         SET status = 'CLOSED',
             revision = revision + 1,
             updated_by = $1,
             updated_at = $2,
             closed_at = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if closed.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    let snapshot = recruitment_snapshot(transaction, guild_id, recruitment_id).await?;
    Ok((before, snapshot))
}

async fn locked_full_entry_assignments(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    target_count: i16,
) -> Result<Vec<SeriesEntryAssignment>, AttemptError> {
    let joined_count = joined_participant_count(transaction, guild_id, recruitment_id).await?;
    let assignments = sqlx::query_as::<_, EntryDraftAssignmentRow>(
        "SELECT assignment.user_id AS participant_id, assignment.team, assignment.lane
         FROM recruitment_entry_assignments AS assignment
         JOIN recruitment_participants AS participant
           ON participant.guild_id = assignment.guild_id
          AND participant.recruitment_id = assignment.recruitment_id
          AND participant.user_id = assignment.user_id
          AND participant.status = 'JOINED'
         WHERE assignment.guild_id = $1 AND assignment.recruitment_id = $2
         ORDER BY assignment.team,
                  CASE assignment.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END
         FOR UPDATE OF assignment",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    if !has_complete_entry_assignments(joined_count, &assignments, target_count) {
        return Err(AttemptError::EntryDraftIncomplete {
            joined: joined_count,
            assigned: i64::try_from(assignments.len())
                .map_err(|_| AttemptError::InvariantViolation)?,
            target: target_count,
        });
    }

    Ok(assignments
        .into_iter()
        .map(|assignment| SeriesEntryAssignment {
            user_id: assignment.participant_id,
            team: assignment.team,
            lane: assignment.lane,
        })
        .collect())
}

async fn locked_entry_draft_auction_conversion_participants(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    target_count: i16,
    season_id: SeasonId,
    now: OffsetDateTime,
) -> Result<Vec<AuctionConversionParticipant>, AttemptError> {
    let rows = sqlx::query_as::<_, EntryDraftAuctionConversionParticipantRow>(
        "SELECT participant.user_id, assignment.lane AS declared_lane
         FROM recruitment_participants AS participant
         LEFT JOIN recruitment_entry_assignments AS assignment
           ON assignment.guild_id = participant.guild_id
          AND assignment.recruitment_id = participant.recruitment_id
          AND assignment.user_id = participant.user_id
         WHERE participant.guild_id = $1
           AND participant.recruitment_id = $2
           AND participant.status = 'JOINED'
         ORDER BY participant.joined_at, participant.user_id
         FOR UPDATE OF participant",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;

    let joined_count = i64::try_from(rows.len()).map_err(|_| AttemptError::InvariantViolation)?;
    let declared_lane_count = rows
        .iter()
        .filter(|row| row.declared_lane.is_some())
        .count();
    if target_count != RECRUITMENT_TARGET_COUNT || joined_count != i64::from(target_count) {
        return Err(AttemptError::EntryDraftIncomplete {
            joined: joined_count,
            assigned: i64::try_from(declared_lane_count)
                .map_err(|_| AttemptError::InvariantViolation)?,
            target: target_count,
        });
    }

    let mmr_rows = mmr::ensure_and_lock_mmr_rows(
        transaction,
        guild_id,
        season_id,
        rows.iter().filter_map(|row| {
            row.declared_lane.map(|lane| MmrLaneKey {
                user_id: row.user_id.clone(),
                lane,
            })
        }),
        now,
    )
    .await?;
    let mmr_by_assignment = mmr_rows
        .into_iter()
        .map(|row| ((row.key.user_id, row.key.lane), row.mmr))
        .collect::<HashMap<_, _>>();
    if mmr_by_assignment.len() != declared_lane_count {
        return Err(AttemptError::InvariantViolation);
    }

    rows.into_iter()
        .map(|row| {
            let mmr_at_join = row
                .declared_lane
                .and_then(|lane| mmr_by_assignment.get(&(row.user_id.clone(), lane)).copied());
            if row.declared_lane.is_some() && mmr_at_join.is_none() {
                return Err(AttemptError::InvariantViolation);
            }
            Ok(AuctionConversionParticipant {
                user_id: row.user_id,
                declared_lane: row.declared_lane,
                mmr_at_join,
                snapshot_origin: if row.declared_lane.is_some() {
                    "CAPTURED"
                } else {
                    "UNAVAILABLE"
                },
            })
        })
        .collect()
}

async fn active_series_exists_for_source_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<bool, AttemptError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM series
             WHERE guild_id = $1
               AND source_recruitment_id = $2
               AND status <> 'CANCELLED'
               AND deleted_at IS NULL
         )",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(AttemptError::Sql)
}

fn has_complete_entry_assignments(
    joined_count: i64,
    assignments: &[EntryDraftAssignmentRow],
    target_count: i16,
) -> bool {
    let expected_count = usize::try_from(target_count).ok();
    if target_count != RECRUITMENT_TARGET_COUNT
        || joined_count != i64::from(target_count)
        || expected_count != Some(assignments.len())
    {
        return false;
    }

    let participant_ids = assignments
        .iter()
        .map(|assignment| assignment.participant_id.clone())
        .collect::<HashSet<_>>();
    if participant_ids.len() != assignments.len() {
        return false;
    }

    let slots = assignments
        .iter()
        .map(|assignment| (assignment.team.as_str(), assignment.lane))
        .collect::<HashSet<_>>();
    let required_slots = [
        ("TEAM_1", Lane::Top),
        ("TEAM_1", Lane::Jungle),
        ("TEAM_1", Lane::Mid),
        ("TEAM_1", Lane::Bottom),
        ("TEAM_1", Lane::Support),
        ("TEAM_2", Lane::Top),
        ("TEAM_2", Lane::Jungle),
        ("TEAM_2", Lane::Mid),
        ("TEAM_2", Lane::Bottom),
        ("TEAM_2", Lane::Support),
    ]
    .into_iter()
    .collect::<HashSet<_>>();

    slots == required_slots
}

async fn apply_entry_draft_operation(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    operation: &EntryDraftOperation,
) -> Result<(), AttemptError> {
    match operation {
        EntryDraftOperation::MoveParticipant {
            participant_id,
            slot,
        } => {
            let is_joined = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (
                     SELECT 1
                     FROM recruitment_participants
                     WHERE guild_id = $1
                       AND recruitment_id = $2
                       AND user_id = $3
                       AND status = 'JOINED'
                 )",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .bind(participant_id)
            .fetch_one(&mut **transaction)
            .await?;
            if !is_joined {
                return Err(AttemptError::ParticipantNotJoined);
            }

            let previous = sqlx::query_as::<_, EntryDraftAssignmentRow>(
                "SELECT user_id AS participant_id, team, lane
                 FROM recruitment_entry_assignments
                 WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3
                 FOR UPDATE",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .bind(participant_id)
            .fetch_optional(&mut **transaction)
            .await?;

            let Some((team, lane)) = slot.as_deref().map(parse_entry_slot).transpose()? else {
                sqlx::query(
                    "DELETE FROM recruitment_entry_assignments
                     WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
                )
                .bind(guild_id)
                .bind(recruitment_id.as_uuid())
                .bind(participant_id)
                .execute(&mut **transaction)
                .await?;
                return Ok(());
            };

            let occupant = sqlx::query_as::<_, EntryDraftAssignmentRow>(
                "SELECT user_id AS participant_id, team, lane
                 FROM recruitment_entry_assignments
                 WHERE guild_id = $1
                   AND recruitment_id = $2
                   AND team = $3
                   AND lane = $4
                 FOR UPDATE",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .bind(team)
            .bind(lane)
            .fetch_optional(&mut **transaction)
            .await?;

            sqlx::query(
                "DELETE FROM recruitment_entry_assignments
                 WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .bind(participant_id)
            .execute(&mut **transaction)
            .await?;

            if let Some(occupant) =
                occupant.filter(|occupant| occupant.participant_id != *participant_id)
            {
                if let Some(previous) = previous.as_ref() {
                    sqlx::query(
                        "UPDATE recruitment_entry_assignments
                         SET team = $1, lane = $2
                         WHERE guild_id = $3 AND recruitment_id = $4 AND user_id = $5",
                    )
                    .bind(&previous.team)
                    .bind(previous.lane)
                    .bind(guild_id)
                    .bind(recruitment_id.as_uuid())
                    .bind(&occupant.participant_id)
                    .execute(&mut **transaction)
                    .await?;
                } else {
                    sqlx::query(
                        "DELETE FROM recruitment_entry_assignments
                         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
                    )
                    .bind(guild_id)
                    .bind(recruitment_id.as_uuid())
                    .bind(&occupant.participant_id)
                    .execute(&mut **transaction)
                    .await?;
                }
            }

            sqlx::query(
                "INSERT INTO recruitment_entry_assignments (
                     guild_id, recruitment_id, user_id, team, lane
                 ) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .bind(participant_id)
            .bind(team)
            .bind(lane)
            .execute(&mut **transaction)
            .await?;
            Ok(())
        }
    }
}

fn parse_entry_slot(slot: &str) -> Result<(&'static str, Lane), AttemptError> {
    for (prefix, team) in [("TEAM_1_", "TEAM_1"), ("TEAM_2_", "TEAM_2")] {
        if let Some(lane) = slot.strip_prefix(prefix) {
            return lane
                .parse::<Lane>()
                .map(|lane| (team, lane))
                .map_err(|_| AttemptError::InvalidEntrySlot);
        }
    }
    Err(AttemptError::InvalidEntrySlot)
}

async fn bump_entry_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let draft = sqlx::query(
        "UPDATE recruitment_entry_drafts
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND recruitment_id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    let recruitment = sqlx::query(
        "UPDATE recruitments
         SET updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if draft.rows_affected() != 1 || recruitment.rows_affected() != 1 {
        return Err(AttemptError::MissingEntryDraft);
    }
    Ok(())
}

async fn bump_recruitment_and_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let recruitment = sqlx::query(
        "UPDATE recruitments
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut **transaction)
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
    .execute(&mut **transaction)
    .await?;
    if recruitment.rows_affected() != 1 || draft.rows_affected() != 1 {
        return Err(AttemptError::MissingEntryDraft);
    }
    Ok(())
}

async fn joined_participant_count(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<i64, AttemptError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM recruitment_participants
         WHERE guild_id = $1 AND recruitment_id = $2 AND status = 'JOINED'",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn joined_auction_recruitment_participant_count(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: AuctionRecruitmentId,
) -> Result<i64, AttemptError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM auction_recruitment_participants
         WHERE guild_id = $1 AND recruitment_id = $2 AND status = 'JOINED'",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn next_series_display_number(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE guild_counters
         SET next_value = next_value + 1
         WHERE guild_id = $1 AND counter_key = 'SERIES'
         RETURNING next_value - 1",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingSeriesCounter)
}

async fn next_auction_recruitment_display_number(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE guild_counters
         SET next_value = next_value + 1
         WHERE guild_id = $1 AND counter_key = 'AUCTION_RECRUITMENT'
         RETURNING next_value - 1",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingAuctionRecruitmentCounter)
}

async fn next_auction_tournament_display_number(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE guild_counters
         SET next_value = next_value + 1
         WHERE guild_id = $1 AND counter_key = 'AUCTION_TOURNAMENT'
         RETURNING next_value - 1",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingAuctionTournamentCounter)
}

async fn insert_audit(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    record: AuditInsert<'_>,
) -> Result<(), AttemptError> {
    sqlx::query(
        "INSERT INTO admin_audit_log (
             guild_id, id, principal_type, principal_id, action, target_type, target_id,
             checkpoint_id, request_id, before_summary, after_summary, result, note, created_at
         ) VALUES ($1, $2, 'USER', $3, $4, $5, $6, NULL, $7, $8, $9, 'APPLIED', NULL, $10)",
    )
    .bind(guild_id)
    .bind(record.id)
    .bind(record.actor_id)
    .bind(record.action)
    .bind(record.target_type)
    .bind(record.target_id)
    .bind(record.request_id)
    .bind(record.before_summary)
    .bind(record.after_summary)
    .bind(record.now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_outbox(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    event_id: OutboxEventId,
    topic: String,
    aggregate_type: &str,
    aggregate_id: &str,
    aggregate_revision: i64,
    event_type: &str,
    payload: Value,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "INSERT INTO outbox_events (
             id, guild_id, topic, aggregate_type, aggregate_id, aggregate_revision,
             event_type, payload, occurred_at, available_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)",
    )
    .bind(event_id.as_uuid())
    .bind(guild_id)
    .bind(topic)
    .bind(aggregate_type)
    .bind(aggregate_id)
    .bind(aggregate_revision)
    .bind(event_type)
    .bind(payload)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn acquire_idempotency<T: DeserializeOwned>(
    transaction: &mut DatabaseTransaction,
    request: IdempotencyAcquire<'_>,
) -> Result<IdempotencyDecision<T>, AttemptError> {
    let command = AcquireIdempotency {
        record_id: request.record_id,
        scope: request.scope,
        principal_type: request.principal_type,
        principal_id: request.principal_id,
        method: request.method,
        route_template: request.route_template,
        idempotency_key: request.key,
        request_hash: request.request_hash,
        created_at: request.now,
        expires_at: request.now + IDEMPOTENCY_TTL,
    };
    match idempotency::acquire(transaction, &command).await? {
        AcquireIdempotencyOutcome::Acquired { record_id } => {
            Ok(IdempotencyDecision::Acquired(record_id))
        }
        AcquireIdempotencyOutcome::Replay { response, .. } => {
            let body = response
                .body
                .ok_or(AttemptError::CorruptIdempotentResponse)?;
            Ok(IdempotencyDecision::Replay(serde_json::from_value(body)?))
        }
        AcquireIdempotencyOutcome::Conflict { .. } => Err(AttemptError::IdempotencyConflict),
        AcquireIdempotencyOutcome::InProgress => Err(AttemptError::IdempotencyInProgress),
    }
}

async fn complete_idempotency<T: Serialize>(
    transaction: &mut DatabaseTransaction,
    scope: &TenantScope,
    record_id: IdempotencyRecordId,
    status_code: u16,
    result: &T,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let outcome = idempotency::complete(
        transaction,
        CompleteIdempotency {
            scope,
            record_id,
            response: IdempotentResponse {
                status_code,
                body: Some(serde_json::to_value(result)?),
            },
            completed_at: now,
        },
    )
    .await?;
    if outcome != CompleteIdempotencyOutcome::Completed {
        return Err(AttemptError::IdempotencyCompletionLost);
    }
    Ok(())
}

fn validate_setup_command(command: &SetupGuildCommand) -> Result<(), BotWorkflowError> {
    validate_actor(&command.actor)?;
    if command.guild_name.trim().is_empty() {
        return Err(BotWorkflowError::InvalidCommand("guild name is empty"));
    }
    if command.season_name.trim().is_empty() {
        return Err(BotWorkflowError::InvalidCommand("season name is empty"));
    }
    Ok(())
}

fn validate_registration_input(input: &RegisterGuildMemberInput) -> Result<(), BotWorkflowError> {
    validate_actor(&input.actor)?;
    if input.requested_game_name.trim().is_empty()
        || input.requested_tag_line.trim().is_empty()
        || input.requested_platform.trim().is_empty()
    {
        return Err(BotWorkflowError::InvalidCommand(
            "Riot account request is empty",
        ));
    }
    Ok(())
}

fn validate_recruitment_command(command: &BotRecruitmentCommand) -> Result<(), BotWorkflowError> {
    validate_actor(command.actor())?;
    if let BotRecruitmentCommand::Create { rules, .. } = command {
        validate_recruitment_series_rules(rules)?;
    }
    if let BotRecruitmentCommand::Join { participant, .. } = command {
        validate_actor(participant)?;
    }
    if let BotRecruitmentCommand::SetPreferredLanes { lanes, .. } = command {
        validate_lanes(lanes)?;
    }
    Ok(())
}

fn validate_auction_recruitment_participation_command(
    command: &BotAuctionRecruitmentParticipationCommand,
) -> Result<(), BotWorkflowError> {
    validate_actor(command.actor())?;
    if let BotAuctionRecruitmentParticipationCommand::Join { participant, .. } = command {
        validate_actor(participant)?;
    }
    Ok(())
}

fn validate_recruitment_series_rules(
    rules: &RecruitmentSeriesRules,
) -> Result<(), BotWorkflowError> {
    if matches!(rules.best_of, 1 | 3 | 5) {
        Ok(())
    } else {
        Err(BotWorkflowError::InvalidRecruitmentBestOf {
            received: rules.best_of,
        })
    }
}

fn validate_auction_recruitment_series_rules(
    format: AuctionFormat,
    rules: &AuctionRecruitmentSeriesRules,
) -> Result<(), BotWorkflowError> {
    if !matches!(rules.final_best_of, 1 | 3 | 5) {
        return Err(BotWorkflowError::InvalidAuctionRecruitmentBestOf {
            stage: "final",
            received: rules.final_best_of,
        });
    }

    match (format, rules.semifinal_best_of) {
        (AuctionFormat::TenPerson, None) => Ok(()),
        (AuctionFormat::TenPerson, Some(_)) => {
            Err(BotWorkflowError::AuctionRecruitmentSemifinalBestOfUnsupported)
        }
        (AuctionFormat::TwentyPerson, Some(best_of)) if matches!(best_of, 1 | 3 | 5) => Ok(()),
        (AuctionFormat::TwentyPerson, Some(best_of)) => {
            Err(BotWorkflowError::InvalidAuctionRecruitmentBestOf {
                stage: "semifinal",
                received: best_of,
            })
        }
        (AuctionFormat::TwentyPerson, None) => {
            Err(BotWorkflowError::AuctionRecruitmentSemifinalBestOfRequired)
        }
    }
}

fn validate_auction_tournament_cancel_command(
    command: &AuctionTournamentCancelCommand,
) -> Result<(), BotWorkflowError> {
    validate_actor(&command.actor)?;
    if command.expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected auction tournament revision must be non-negative",
        ));
    }
    let reason = command.reason.trim();
    if reason.is_empty() || reason.chars().count() > MAX_AUCTION_TOURNAMENT_CANCEL_REASON_CHARS {
        return Err(BotWorkflowError::InvalidCommand(
            "auction tournament cancellation reason is invalid",
        ));
    }
    Ok(())
}

fn validate_web_auction_tournament_cancel_command(
    command: &WebAuctionTournamentCancelCommand,
) -> Result<(), BotWorkflowError> {
    if command.expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected auction tournament revision must be non-negative",
        ));
    }
    let reason = command.reason.trim();
    if reason.is_empty() || reason.chars().count() > MAX_AUCTION_TOURNAMENT_CANCEL_REASON_CHARS {
        return Err(BotWorkflowError::InvalidCommand(
            "auction tournament cancellation reason is invalid",
        ));
    }
    Ok(())
}

fn validate_actor(actor: &BotActor) -> Result<(), BotWorkflowError> {
    if actor.global_display_name.trim().is_empty() || actor.display_name.trim().is_empty() {
        return Err(BotWorkflowError::InvalidCommand(
            "actor display name is empty",
        ));
    }
    Ok(())
}

fn validate_lanes(lanes: &[Lane]) -> Result<(), BotWorkflowError> {
    if lanes.is_empty() || lanes.len() > MAX_PREFERRED_LANES {
        return Err(BotWorkflowError::InvalidPreferredLanes);
    }
    let unique: HashSet<Lane> = lanes.iter().copied().collect();
    if unique.len() != lanes.len() {
        return Err(BotWorkflowError::InvalidPreferredLanes);
    }
    Ok(())
}

fn validate_series_pick_ban_operation(
    operation: &SeriesPickBanOperation,
) -> Result<(), BotWorkflowError> {
    let valid = match operation {
        SeriesPickBanOperation::SetSide { side_preset } => {
            matches!(side_preset.as_str(), "DEFAULT" | "SWAPPED")
        }
        SeriesPickBanOperation::SetPick { champion_id, .. } => is_valid_champion_id(champion_id),
        SeriesPickBanOperation::ClearPick { .. } => true,
        SeriesPickBanOperation::SetBan {
            side,
            slot,
            champion_id,
        } => {
            matches!(side.as_str(), "BLUE" | "RED")
                && (1..=5).contains(slot)
                && is_valid_champion_id(champion_id)
        }
        SeriesPickBanOperation::ClearBan { side, slot } => {
            matches!(side.as_str(), "BLUE" | "RED") && (1..=5).contains(slot)
        }
        SeriesPickBanOperation::ApplyBulk { items } => validate_series_pick_ban_bulk_items(items),
    };
    if valid {
        Ok(())
    } else {
        Err(BotWorkflowError::InvalidCommand(
            "series pick-ban operation is invalid",
        ))
    }
}

fn validate_series_pick_ban_bulk_items(items: &[SeriesPickBanBulkItem]) -> bool {
    if items.is_empty() || items.len() > 20 {
        return false;
    }

    let mut pick_players = HashSet::with_capacity(items.len());
    let mut ban_slots = HashSet::with_capacity(items.len());
    for item in items {
        match item {
            SeriesPickBanBulkItem::SetPick {
                player_id,
                champion_id,
            } => {
                if !is_valid_champion_id(champion_id) || !pick_players.insert(player_id) {
                    return false;
                }
            }
            SeriesPickBanBulkItem::SetBan {
                side,
                slot,
                champion_id,
            } => {
                if !matches!(side.as_str(), "BLUE" | "RED")
                    || !(1..=5).contains(slot)
                    || !is_valid_champion_id(champion_id)
                    || !ban_slots.insert((side.as_str(), *slot))
                {
                    return false;
                }
            }
        }
    }
    true
}

fn validate_series_game_submit_command(
    command: &SeriesGameSubmitCommand,
) -> Result<(), BotWorkflowError> {
    if command.expected_revision < 0
        || !(1..=5).contains(&command.set_number)
        || !matches!(command.side_preset.as_str(), "DEFAULT" | "SWAPPED")
        || !matches!(command.winner.as_str(), "BLUE" | "RED")
        || !is_valid_patch_version(command.patch_version.as_deref())
    {
        return Err(BotWorkflowError::InvalidCommand(
            "series game submit command is invalid",
        ));
    }
    if let Some(picks) = command.picks.as_ref() {
        if picks
            .values()
            .flatten()
            .any(|champion| !is_valid_champion_id(champion))
        {
            return Err(BotWorkflowError::InvalidCommand(
                "series game submit champions are invalid",
            ));
        }
    }
    if let Some(bans) = command.bans.as_ref() {
        if bans.blue.len() != 5
            || bans.red.len() != 5
            || bans
                .blue
                .iter()
                .chain(&bans.red)
                .flatten()
                .any(|champion| !is_valid_champion_id(champion))
        {
            return Err(BotWorkflowError::InvalidCommand(
                "series game submit bans are invalid",
            ));
        }
    }
    Ok(())
}

fn validate_auction_match_game_submit_command(
    command: &AuctionMatchGameSubmitCommand,
) -> Result<(), BotWorkflowError> {
    if command.expected_revision < 0
        || !(1..=5).contains(&command.set_number)
        || !matches!(command.side_preset.as_str(), "DEFAULT" | "SWAPPED")
        || !matches!(command.winner.as_str(), "BLUE" | "RED")
        || !is_valid_patch_version(command.patch_version.as_deref())
        || command
            .picks
            .values()
            .flatten()
            .any(|champion| !is_valid_champion_id(champion))
        || command.bans.blue.len() != 5
        || command.bans.red.len() != 5
        || command
            .bans
            .blue
            .iter()
            .chain(&command.bans.red)
            .flatten()
            .any(|champion| !is_valid_champion_id(champion))
    {
        return Err(BotWorkflowError::InvalidCommand(
            "auction match game submit command is invalid",
        ));
    }
    Ok(())
}

fn is_valid_champion_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CHAMPION_ID_BYTES
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_valid_patch_version(value: Option<&str>) -> bool {
    value.is_none_or(|value| !value.is_empty() && value.len() <= MAX_PATCH_VERSION_BYTES)
}

fn validate_champion_id(value: &str) -> Result<(), AttemptError> {
    is_valid_champion_id(value)
        .then_some(())
        .ok_or(AttemptError::InvalidGameResult)
}

fn validate_patch_version(value: Option<&str>) -> Result<(), AttemptError> {
    is_valid_patch_version(value)
        .then_some(())
        .ok_or(AttemptError::InvalidGameResult)
}

fn ensure_roster_player(roster: &[SeriesRosterRow], user_id: &UserId) -> Result<(), AttemptError> {
    roster
        .iter()
        .any(|player| player.user_id == *user_id)
        .then_some(())
        .ok_or(AttemptError::SeriesPickBanPlayerNotInRoster)
}

fn ensure_ban_slot(slot: i16) -> Result<(), AttemptError> {
    (1..=5)
        .contains(&slot)
        .then_some(())
        .ok_or(AttemptError::InvalidGameResult)
}

fn validate_submit_picks(
    picks: &BTreeMap<UserId, Option<String>>,
    roster: &[SeriesRosterRow],
) -> Result<(), AttemptError> {
    let expected = roster
        .iter()
        .map(|player| player.user_id.clone())
        .collect::<BTreeSet<_>>();
    let actual = picks.keys().cloned().collect::<BTreeSet<_>>();
    if actual != expected
        || picks
            .values()
            .flatten()
            .any(|champion| !is_valid_champion_id(champion))
    {
        return Err(AttemptError::InvalidGameResult);
    }
    Ok(())
}

fn validate_submit_bans(bans: &SeriesPickBanBansSnapshot) -> Result<(), AttemptError> {
    if bans.blue.len() != 5
        || bans.red.len() != 5
        || bans
            .blue
            .iter()
            .chain(&bans.red)
            .flatten()
            .any(|champion| !is_valid_champion_id(champion))
    {
        return Err(AttemptError::InvalidGameResult);
    }
    Ok(())
}

fn require_operator(
    actor: &BotActor,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    if actor.role_ids.iter().any(|role| role == operator_role_id) {
        Ok(())
    } else {
        Err(AttemptError::OperatorRequired)
    }
}

fn require_self_or_operator(
    actor: &BotActor,
    target_user_id: &UserId,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    if UserId::from(&actor.id) == *target_user_id {
        Ok(())
    } else {
        require_operator(actor, operator_role_id)
    }
}

fn require_creator_or_operator(
    actor: &BotActor,
    created_by: &UserId,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    if UserId::from(&actor.id) == *created_by {
        Ok(())
    } else {
        require_operator(actor, operator_role_id)
    }
}

fn ensure_open(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Open {
        Ok(())
    } else {
        Err(AttemptError::RecruitmentNotOpen(status))
    }
}

fn ensure_participant_confirmation_dms_dispatchable(
    status: RecruitmentStatus,
) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Closed {
        Ok(())
    } else {
        Err(AttemptError::ParticipantConfirmationDmUnavailable(status))
    }
}

fn ensure_auction_setup_editable(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Closed {
        Ok(())
    } else {
        Err(AttemptError::AuctionSetupDraftNotEditable(status))
    }
}

fn ensure_auction_tournament_source_ready(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Closed {
        Ok(())
    } else {
        Err(AttemptError::AuctionTournamentSourceNotReady(status))
    }
}

fn ensure_entry_draft_auction_source_ready(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Closed {
        Ok(())
    } else {
        Err(AttemptError::EntryDraftAuctionSourceNotReady(status))
    }
}

fn ensure_full_auction_roster(
    participants: &[AuctionRecruitmentParticipantLockRow],
    target: i16,
) -> Result<(), AttemptError> {
    let joined = i64::try_from(participants.len()).map_err(|_| AttemptError::InvariantViolation)?;
    if joined == i64::from(target) {
        Ok(())
    } else {
        Err(AttemptError::CloseRequiresFullRoster { joined, target })
    }
}

fn ensure_series_source_ready(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Closed {
        Ok(())
    } else {
        Err(AttemptError::SeriesSourceNotReady(status))
    }
}

fn ensure_series_source_converted(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Converted {
        Ok(())
    } else {
        Err(AttemptError::SeriesSourceNotConverted(status))
    }
}

fn ensure_series_in_progress(status: SeriesStatus) -> Result<(), AttemptError> {
    if status == SeriesStatus::InProgress {
        Ok(())
    } else {
        Err(AttemptError::SeriesNotInProgress(status))
    }
}

fn ensure_series_game_revertible(status: SeriesStatus) -> Result<(), AttemptError> {
    if matches!(status, SeriesStatus::InProgress | SeriesStatus::Completed) {
        Ok(())
    } else {
        Err(AttemptError::SeriesNotInProgress(status))
    }
}

fn ensure_cancellable(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if matches!(status, RecruitmentStatus::Open | RecruitmentStatus::Closed) {
        Ok(())
    } else {
        Err(AttemptError::RecruitmentNotOpen(status))
    }
}

fn ensure_entry_editable(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Closed {
        Ok(())
    } else {
        Err(AttemptError::EntryDraftNotEditable(status))
    }
}

fn ensure_reopenable(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Closed {
        Ok(())
    } else {
        Err(AttemptError::EntryDraftNotEditable(status))
    }
}

fn ensure_bindable(status: RecruitmentStatus) -> Result<(), AttemptError> {
    match status {
        RecruitmentStatus::Open | RecruitmentStatus::Closed => Ok(()),
        RecruitmentStatus::Converted | RecruitmentStatus::Cancelled => {
            Err(AttemptError::RecruitmentNotBindable(status))
        }
    }
}

fn parse_auction_format(value: &str) -> Result<AuctionFormat, AttemptError> {
    match value {
        "TEN_PERSON" => Ok(AuctionFormat::TenPerson),
        "TWENTY_PERSON" => Ok(AuctionFormat::TwentyPerson),
        _ => Err(AttemptError::InvariantViolation),
    }
}

fn request_hash<T: Serialize>(
    guild_id: &GuildId,
    command: &T,
) -> Result<Vec<u8>, BotWorkflowError> {
    let canonical = serde_json::to_string(&(guild_id, command))?;
    Ok(AuthService::hash_secret(&canonical))
}

fn attempt_sql_error(error: &AttemptError) -> Option<&sqlx::Error> {
    match error {
        AttemptError::Sql(error) => Some(error),
        AttemptError::Mmr(MmrRepositoryError::Sql(error)) => Some(error),
        AttemptError::Idempotency(IdempotencyRepositoryError::Sql(error)) => Some(error),
        AttemptError::Notification(NotificationOutboxError::Sql(error)) => Some(error),
        AttemptError::Riot(RiotRepositoryError::Sql(error)) => Some(error),
        _ => None,
    }
}

fn map_preflight_error(error: IdempotencyRepositoryError) -> BotWorkflowError {
    match error {
        IdempotencyRepositoryError::Sql(error) => BotWorkflowError::Database(error),
        _ => BotWorkflowError::InvariantViolation,
    }
}

fn map_transaction_error(error: TransactionRetryError<AttemptError>) -> BotWorkflowError {
    match error {
        TransactionRetryError::DeadlockRetriesExhausted { .. } => BotWorkflowError::DatabaseBusy,
        TransactionRetryError::NonRetryable { failure, .. } => match failure {
            TransactionAttemptError::Begin(error) | TransactionAttemptError::Commit(error) => {
                BotWorkflowError::Database(error)
            }
            TransactionAttemptError::Operation(error) => error.into_public(),
        },
    }
}

struct IdempotencyAcquire<'a> {
    scope: &'a TenantScope,
    record_id: IdempotencyRecordId,
    principal_type: IdempotencyPrincipalType,
    principal_id: &'a str,
    method: &'a str,
    route_template: &'a str,
    key: &'a str,
    request_hash: &'a [u8],
    now: OffsetDateTime,
}

struct AuditInsert<'a> {
    id: Uuid,
    actor_id: &'a str,
    action: &'a str,
    target_type: &'a str,
    target_id: &'a str,
    request_id: &'a str,
    before_summary: Option<Value>,
    after_summary: Option<Value>,
    now: OffsetDateTime,
}

#[derive(Clone, Copy)]
struct SetupPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    season_id: SeasonId,
    audit_id: Uuid,
}

#[derive(Clone, Copy)]
struct RegistrationPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
}

impl RegistrationPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
        }
    }
}

impl SetupPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            season_id: SeasonId::new(),
            audit_id: Uuid::now_v7(),
        }
    }
}

#[derive(Clone, Copy)]
struct RecruitmentPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    recruitment_id: RecruitmentId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    notification_id: NotificationId,
}

impl RecruitmentPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            recruitment_id: RecruitmentId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct AuctionRecruitmentPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    recruitment_id: AuctionRecruitmentId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    notification_id: NotificationId,
}

#[derive(Clone, Copy)]
struct AuctionRecruitmentMutationPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    notification_id: NotificationId,
}

impl AuctionRecruitmentMutationPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            notification_id: NotificationId::new(),
        }
    }
}

impl AuctionRecruitmentPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            recruitment_id: AuctionRecruitmentId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebAuctionRecruitmentLifecyclePrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    notification_id: NotificationId,
}

impl WebAuctionRecruitmentLifecyclePrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebParticipantConfirmationDmDispatchPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
}

impl WebParticipantConfirmationDmDispatchPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebAuctionRecruitmentMutationPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    notification_id: NotificationId,
}

impl WebAuctionRecruitmentMutationPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebAuctionSetupDraftPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
}

impl WebAuctionSetupDraftPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebAuctionTournamentCreatePrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    tournament_id: AuctionTournamentId,
    team_ids: [Uuid; 4],
    team_member_ids: [Uuid; 4],
    point_ledger_ids: [Uuid; 4],
    audit_id: Uuid,
    recruitment_outbox_event_id: OutboxEventId,
    tournament_outbox_event_id: OutboxEventId,
}

impl WebAuctionTournamentCreatePrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            tournament_id: AuctionTournamentId::new(),
            team_ids: std::array::from_fn(|_| Uuid::now_v7()),
            team_member_ids: std::array::from_fn(|_| Uuid::now_v7()),
            point_ledger_ids: std::array::from_fn(|_| Uuid::now_v7()),
            audit_id: Uuid::now_v7(),
            recruitment_outbox_event_id: OutboxEventId::new(),
            tournament_outbox_event_id: OutboxEventId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebAuctionTournamentDraftPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    action_id: Uuid,
    lot_id: Uuid,
    team_member_id: Uuid,
    point_ledger_id: Uuid,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
}

impl WebAuctionTournamentDraftPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            action_id: Uuid::now_v7(),
            lot_id: Uuid::now_v7(),
            team_member_id: Uuid::now_v7(),
            point_ledger_id: Uuid::now_v7(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebAuctionTournamentBracketPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    match_ids: [Uuid; 3],
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuctionTournamentBracketCommandOutcome {
    Updated,
    Locked,
    Unlocked,
}

impl WebAuctionTournamentBracketPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            match_ids: std::array::from_fn(|_| Uuid::now_v7()),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct AuctionTournamentCancelPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    tournament_outbox_event_id: OutboxEventId,
    recruitment_outbox_event_id: OutboxEventId,
    recruitment_notification_id: NotificationId,
}

impl AuctionTournamentCancelPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            tournament_outbox_event_id: OutboxEventId::new(),
            recruitment_outbox_event_id: OutboxEventId::new(),
            recruitment_notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct AuctionRecruitmentForceCancelPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    recruitment_outbox_event_id: OutboxEventId,
    tournament_outbox_event_id: OutboxEventId,
    recruitment_notification_id: NotificationId,
}

impl AuctionRecruitmentForceCancelPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            recruitment_outbox_event_id: OutboxEventId::new(),
            tournament_outbox_event_id: OutboxEventId::new(),
            recruitment_notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebAuctionMatchLineupPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    lineup_confirmation_notification_id: NotificationId,
}

impl WebAuctionMatchLineupPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            lineup_confirmation_notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebAuctionMatchGameMutationPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    game_id: GameId,
    input_id: Uuid,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    result_notification_id: NotificationId,
}

impl WebAuctionMatchGameMutationPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            game_id: GameId::new(),
            input_id: Uuid::now_v7(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            result_notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebRecruitmentMutationPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    notification_id: NotificationId,
}

impl WebRecruitmentMutationPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebEntryDraftMutationPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    recruitment_outbox_event_id: OutboxEventId,
    notification_id: NotificationId,
}

impl WebEntryDraftMutationPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            recruitment_outbox_event_id: OutboxEventId::new(),
            notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebEntryDraftAuctionConversionPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    auction_recruitment_id: AuctionRecruitmentId,
    audit_id: Uuid,
    regular_recruitment_outbox_event_id: OutboxEventId,
    auction_recruitment_outbox_event_id: OutboxEventId,
    regular_recruitment_notification_id: NotificationId,
    auction_recruitment_notification_id: NotificationId,
}

impl WebEntryDraftAuctionConversionPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            auction_recruitment_id: AuctionRecruitmentId::new(),
            audit_id: Uuid::now_v7(),
            regular_recruitment_outbox_event_id: OutboxEventId::new(),
            auction_recruitment_outbox_event_id: OutboxEventId::new(),
            regular_recruitment_notification_id: NotificationId::new(),
            auction_recruitment_notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebSeriesCreatePrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    series_id: SeriesId,
    audit_id: Uuid,
    recruitment_outbox_event_id: OutboxEventId,
    series_outbox_event_id: OutboxEventId,
    recruitment_message_notification_id: NotificationId,
    entry_confirmation_notification_id: NotificationId,
}

impl WebSeriesCreatePrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            series_id: SeriesId::new(),
            audit_id: Uuid::now_v7(),
            recruitment_outbox_event_id: OutboxEventId::new(),
            series_outbox_event_id: OutboxEventId::new(),
            recruitment_message_notification_id: NotificationId::new(),
            entry_confirmation_notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebSeriesLifecyclePrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    recruitment_outbox_event_id: OutboxEventId,
    series_outbox_event_id: OutboxEventId,
}

impl WebSeriesLifecyclePrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            recruitment_outbox_event_id: OutboxEventId::new(),
            series_outbox_event_id: OutboxEventId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebSeriesPickBanPrepared {
    now: OffsetDateTime,
    input_id: Uuid,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
}

impl WebSeriesPickBanPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            input_id: Uuid::now_v7(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebSeriesGameMutationPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    game_id: GameId,
    input_id: Uuid,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    result_notification_id: NotificationId,
}

impl WebSeriesGameMutationPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            game_id: GameId::new(),
            input_id: Uuid::now_v7(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            result_notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebSeriesEarlyCompletePrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    result_notification_id: NotificationId,
}

impl WebSeriesEarlyCompletePrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            result_notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct CommandMetadata {
    method: &'static str,
    route_template: &'static str,
    success_status: u16,
    audit_action: &'static str,
    event_type: &'static str,
}

impl BotRecruitmentCommand {
    fn actor(&self) -> &BotActor {
        match self {
            Self::Create { actor, .. }
            | Self::BindMessage { actor, .. }
            | Self::Join { actor, .. }
            | Self::Leave { actor, .. }
            | Self::SetPreferredLanes { actor, .. }
            | Self::Close { actor, .. } => actor,
        }
    }

    fn metadata(&self) -> CommandMetadata {
        match self {
            Self::Create { .. } => CommandMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments",
                success_status: 201,
                audit_action: "RECRUITMENT_CREATE",
                event_type: "RECRUITMENT_CREATED",
            },
            Self::BindMessage { .. } => CommandMetadata {
                method: "PUT",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/message",
                success_status: 200,
                audit_action: "RECRUITMENT_BIND_MESSAGE",
                event_type: "RECRUITMENT_MESSAGE_BOUND",
            },
            Self::Join { .. } => CommandMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/members",
                success_status: 200,
                audit_action: "RECRUITMENT_JOIN",
                event_type: "RECRUITMENT_PARTICIPANT_JOINED",
            },
            Self::Leave { .. } => CommandMetadata {
                method: "DELETE",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/members/{userId}",
                success_status: 200,
                audit_action: "RECRUITMENT_LEAVE",
                event_type: "RECRUITMENT_PARTICIPANT_LEFT",
            },
            Self::SetPreferredLanes { .. } => CommandMetadata {
                method: "PUT",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/members/{userId}/preferred-lanes",
                success_status: 200,
                audit_action: "RECRUITMENT_SET_PREFERRED_LANES",
                event_type: "RECRUITMENT_PREFERRED_LANES_SET",
            },
            Self::Close { .. } => CommandMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/close",
                success_status: 200,
                audit_action: "RECRUITMENT_CLOSE",
                event_type: "RECRUITMENT_CLOSED",
            },
        }
    }
}

impl BotAuctionRecruitmentParticipationCommand {
    fn actor(&self) -> &BotActor {
        match self {
            Self::Join { actor, .. } | Self::Leave { actor, .. } => actor,
        }
    }

    fn metadata(&self) -> CommandMetadata {
        match self {
            Self::Join { .. } => CommandMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/auction-recruitments/{id}/members",
                success_status: 200,
                audit_action: "AUCTION_RECRUITMENT_JOIN",
                event_type: "AUCTION_RECRUITMENT_PARTICIPANT_JOINED",
            },
            Self::Leave { .. } => CommandMetadata {
                method: "DELETE",
                route_template: "/api/guilds/{guildId}/auction-recruitments/{id}/members/{userId}",
                success_status: 200,
                audit_action: "AUCTION_RECRUITMENT_LEAVE",
                event_type: "AUCTION_RECRUITMENT_PARTICIPANT_LEFT",
            },
        }
    }
}

enum IdempotencyDecision<T> {
    Acquired(IdempotencyRecordId),
    Replay(T),
}

#[derive(Clone)]
enum RecruitmentCreateActor {
    Bot(BotActor),
    Session(UserId),
}

#[derive(Clone)]
enum AuctionRecruitmentCreateActor {
    Bot(BotActor),
    Session(UserId),
}

impl AuctionRecruitmentCreateActor {
    fn user_id(&self) -> UserId {
        match self {
            Self::Bot(actor) => UserId::from(&actor.id),
            Self::Session(user_id) => user_id.clone(),
        }
    }
}

impl RecruitmentCreateActor {
    fn user_id(&self) -> UserId {
        match self {
            Self::Bot(actor) => UserId::from(&actor.id),
            Self::Session(user_id) => user_id.clone(),
        }
    }
}

#[derive(Serialize)]
struct WebRecruitmentCreateRequest {
    rules: RecruitmentSeriesRules,
    #[serde(with = "time::serde::rfc3339")]
    expected_start_at: OffsetDateTime,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionRecruitmentCreateRequest {
    format: AuctionFormat,
    rules: AuctionRecruitmentSeriesRules,
    #[serde(with = "time::serde::rfc3339")]
    expected_start_at: OffsetDateTime,
}

#[derive(Clone, Serialize)]
enum WebAuctionRecruitmentLifecycle {
    Close {
        recruitment_id: AuctionRecruitmentId,
    },
    Cancel {
        recruitment_id: AuctionRecruitmentId,
    },
}

#[derive(Clone, Serialize)]
struct WebAuctionRecruitmentParticipantConfirmationDmDispatchCommand {
    recruitment_id: AuctionRecruitmentId,
}

impl WebAuctionRecruitmentLifecycle {
    fn metadata(&self) -> WebAuctionRecruitmentLifecycleMetadata {
        match self {
            Self::Close { .. } => WebAuctionRecruitmentLifecycleMetadata {
                method: "POST",
                route_template: AUCTION_RECRUITMENT_CLOSE_ROUTE,
                audit_action: "AUCTION_RECRUITMENT_CLOSED",
                event_type: "AUCTION_RECRUITMENT_CLOSED",
            },
            Self::Cancel { .. } => WebAuctionRecruitmentLifecycleMetadata {
                method: "DELETE",
                route_template: AUCTION_RECRUITMENT_CANCEL_ROUTE,
                audit_action: "AUCTION_RECRUITMENT_CANCELLED",
                event_type: "AUCTION_RECRUITMENT_CANCELLED",
            },
        }
    }
}

#[derive(Clone, Copy)]
struct WebAuctionRecruitmentLifecycleMetadata {
    method: &'static str,
    route_template: &'static str,
    audit_action: &'static str,
    event_type: &'static str,
}

#[derive(Clone, Serialize)]
enum WebAuctionRecruitmentMutation {
    AddMember {
        recruitment_id: AuctionRecruitmentId,
        member_id: UserId,
        declared_lane: Lane,
    },
    RemoveMember {
        recruitment_id: AuctionRecruitmentId,
        member_id: UserId,
    },
}

impl WebAuctionRecruitmentMutation {
    fn metadata(&self) -> WebAuctionRecruitmentMutationMetadata {
        match self {
            Self::AddMember { .. } => WebAuctionRecruitmentMutationMetadata {
                method: "POST",
                route_template: AUCTION_RECRUITMENT_MEMBERS_ROUTE,
                audit_action: "AUCTION_RECRUITMENT_MEMBER_ADDED",
                event_type: "AUCTION_RECRUITMENT_MEMBER_ADDED",
            },
            Self::RemoveMember { .. } => WebAuctionRecruitmentMutationMetadata {
                method: "DELETE",
                route_template: AUCTION_RECRUITMENT_MEMBER_ROUTE,
                audit_action: "AUCTION_RECRUITMENT_MEMBER_REMOVED",
                event_type: "AUCTION_RECRUITMENT_MEMBER_REMOVED",
            },
        }
    }
}

#[derive(Clone, Copy)]
struct WebAuctionRecruitmentMutationMetadata {
    method: &'static str,
    route_template: &'static str,
    audit_action: &'static str,
    event_type: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebAuctionSetupDraftUpdateCommand {
    recruitment_id: AuctionRecruitmentId,
    expected_revision: i64,
    teams: Vec<AuctionSetupDraftTeamInput>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebAuctionTournamentCreateCommand {
    source_recruitment_id: AuctionRecruitmentId,
    expected_revision: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AuctionTournamentDraftCommand {
    DrawNext {
        expected_revision: i64,
    },
    ReshuffleCandidates {
        expected_revision: i64,
    },
    MarkUnsold {
        expected_revision: i64,
        member_id: UserId,
    },
    AwardBid {
        expected_revision: i64,
        member_id: UserId,
        team_id: Uuid,
        points: i32,
    },
    AssignCurrent {
        expected_revision: i64,
        member_id: UserId,
        team_id: Uuid,
    },
    AssignUnsold {
        expected_revision: i64,
        member_id: UserId,
        team_id: Uuid,
    },
    RestoreUnsold {
        expected_revision: i64,
        member_id: UserId,
    },
    ResetToInitial {
        expected_revision: i64,
    },
    RevertLast {
        expected_revision: i64,
    },
}

impl AuctionTournamentDraftCommand {
    pub(crate) fn expected_revision(&self) -> i64 {
        match self {
            Self::DrawNext { expected_revision }
            | Self::ReshuffleCandidates { expected_revision }
            | Self::MarkUnsold {
                expected_revision, ..
            }
            | Self::AwardBid {
                expected_revision, ..
            }
            | Self::AssignCurrent {
                expected_revision, ..
            }
            | Self::AssignUnsold {
                expected_revision, ..
            }
            | Self::RestoreUnsold {
                expected_revision, ..
            }
            | Self::ResetToInitial { expected_revision }
            | Self::RevertLast { expected_revision } => *expected_revision,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::DrawNext { .. } => "DRAW_NEXT",
            Self::ReshuffleCandidates { .. } => "RESHUFFLE_CANDIDATES",
            Self::MarkUnsold { .. } => "MARK_UNSOLD",
            Self::AwardBid { .. } => "AWARD_BID",
            Self::AssignCurrent { .. } => "ASSIGN_CURRENT",
            Self::AssignUnsold { .. } => "ASSIGN_UNSOLD",
            Self::RestoreUnsold { .. } => "RESTORE_UNSOLD",
            Self::ResetToInitial { .. } => "RESET_TO_INITIAL",
            Self::RevertLast { .. } => "REVERT_LAST",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AuctionTournamentBracketCommand {
    AssignTeam {
        expected_revision: i64,
        slot_key: String,
        team_id: Uuid,
    },
    ClearSlot {
        expected_revision: i64,
        slot_key: String,
    },
    LockBracket {
        expected_revision: i64,
    },
    UnlockBracket {
        expected_revision: i64,
    },
}

impl AuctionTournamentBracketCommand {
    pub(crate) fn expected_revision(&self) -> i64 {
        match self {
            Self::AssignTeam {
                expected_revision, ..
            }
            | Self::ClearSlot {
                expected_revision, ..
            }
            | Self::LockBracket { expected_revision }
            | Self::UnlockBracket { expected_revision } => *expected_revision,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::AssignTeam { .. } => "ASSIGN_TEAM",
            Self::ClearSlot { .. } => "CLEAR_SLOT",
            Self::LockBracket { .. } => "LOCK_BRACKET",
            Self::UnlockBracket { .. } => "UNLOCK_BRACKET",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebSeriesCreateCommand {
    source_recruitment_id: RecruitmentId,
    expected_revision: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebEntryDraftAuctionConversionCommand {
    source_recruitment_id: RecruitmentId,
    expected_revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    expected_start_at: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum SeriesLifecycleAction {
    Revert,
    Delete,
}

impl SeriesLifecycleAction {
    fn metadata(self) -> SeriesLifecycleMetadata {
        match self {
            Self::Revert => SeriesLifecycleMetadata {
                method: SERIES_REVERT_METHOD,
                route_template: SERIES_REVERT_ROUTE,
                audit_action: "SERIES_REVERTED",
                series_event_type: "SERIES_REVERTED",
                recruitment_event_type: "RECRUITMENT_RESTORED_FROM_SERIES",
                source_status: "CLOSED",
            },
            Self::Delete => SeriesLifecycleMetadata {
                method: SERIES_DELETE_METHOD,
                route_template: SERIES_DELETE_ROUTE,
                audit_action: "SERIES_CANCELLED",
                series_event_type: "SERIES_CANCELLED",
                recruitment_event_type: "RECRUITMENT_CANCELLED_FROM_SERIES",
                source_status: "CANCELLED",
            },
        }
    }
}

#[derive(Clone, Copy)]
struct SeriesLifecycleMetadata {
    method: &'static str,
    route_template: &'static str,
    audit_action: &'static str,
    series_event_type: &'static str,
    recruitment_event_type: &'static str,
    source_status: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebSeriesLifecycleCommand {
    series_id: SeriesId,
    expected_revision: i64,
    action: SeriesLifecycleAction,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebSeriesPickBanCommand {
    series_id: SeriesId,
    expected_revision: i64,
    operation: SeriesPickBanOperation,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebSeriesGameSubmitCommand {
    series_id: SeriesId,
    #[serde(flatten)]
    command: SeriesGameSubmitCommand,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebSeriesGameRevertCommand {
    series_id: SeriesId,
    #[serde(flatten)]
    command: SeriesGameRevertCommand,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebSeriesEarlyCompleteCommand {
    series_id: SeriesId,
    #[serde(flatten)]
    command: SeriesEarlyCompleteCommand,
}

#[derive(Clone, Serialize)]
enum WebRecruitmentMutation {
    AddMember {
        recruitment_id: RecruitmentId,
        member_id: UserId,
    },
    RemoveMember {
        recruitment_id: RecruitmentId,
        member_id: UserId,
    },
    Close {
        recruitment_id: RecruitmentId,
    },
    Cancel {
        recruitment_id: RecruitmentId,
    },
}

#[derive(Clone, Serialize)]
struct WebRecruitmentParticipantConfirmationDmDispatchCommand {
    recruitment_id: RecruitmentId,
}

impl WebRecruitmentMutation {
    fn metadata(&self) -> WebRecruitmentMutationMetadata {
        match self {
            Self::AddMember { .. } => WebRecruitmentMutationMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/members",
                audit_action: "RECRUITMENT_MEMBER_ADDED",
                event_type: "RECRUITMENT_MEMBER_ADDED",
            },
            Self::RemoveMember { .. } => WebRecruitmentMutationMetadata {
                method: "DELETE",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/members/{memberId}",
                audit_action: "RECRUITMENT_MEMBER_REMOVED",
                event_type: "RECRUITMENT_MEMBER_REMOVED",
            },
            Self::Close { .. } => WebRecruitmentMutationMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/close",
                audit_action: "RECRUITMENT_CLOSE",
                event_type: "RECRUITMENT_CLOSED",
            },
            Self::Cancel { .. } => WebRecruitmentMutationMetadata {
                method: "DELETE",
                route_template: "/api/guilds/{guildId}/recruitments/{id}",
                audit_action: "RECRUITMENT_CANCELLED",
                event_type: "RECRUITMENT_CANCELLED",
            },
        }
    }
}

#[derive(Clone, Serialize)]
enum WebEntryDraftMutation {
    Apply {
        recruitment_id: RecruitmentId,
        expected_revision: i64,
        operation: EntryDraftOperation,
    },
    Reset {
        recruitment_id: RecruitmentId,
        expected_revision: i64,
    },
    Reopen {
        recruitment_id: RecruitmentId,
        expected_revision: i64,
    },
}

impl WebEntryDraftMutation {
    fn recruitment_id(&self) -> RecruitmentId {
        match self {
            Self::Apply { recruitment_id, .. }
            | Self::Reset { recruitment_id, .. }
            | Self::Reopen { recruitment_id, .. } => *recruitment_id,
        }
    }

    fn expected_revision(&self) -> i64 {
        match self {
            Self::Apply {
                expected_revision, ..
            }
            | Self::Reset {
                expected_revision, ..
            }
            | Self::Reopen {
                expected_revision, ..
            } => *expected_revision,
        }
    }

    fn metadata(&self) -> WebEntryDraftMutationMetadata {
        match self {
            Self::Apply { .. } => WebEntryDraftMutationMetadata {
                method: "PATCH",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/entry-draft",
                audit_action: "RECRUITMENT_ENTRY_DRAFT_UPDATED",
                event_type: "RECRUITMENT_ENTRY_DRAFT_UPDATED",
            },
            Self::Reset { .. } => WebEntryDraftMutationMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/entry-draft/reset",
                audit_action: "RECRUITMENT_ENTRY_DRAFT_RESET",
                event_type: "RECRUITMENT_ENTRY_DRAFT_RESET",
            },
            Self::Reopen { .. } => WebEntryDraftMutationMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/reopen",
                audit_action: "RECRUITMENT_REOPENED",
                event_type: "RECRUITMENT_REOPENED",
            },
        }
    }
}

#[derive(Clone, Copy)]
struct WebRecruitmentMutationMetadata {
    method: &'static str,
    route_template: &'static str,
    audit_action: &'static str,
    event_type: &'static str,
}

#[derive(Clone, Copy)]
struct WebEntryDraftMutationMetadata {
    method: &'static str,
    route_template: &'static str,
    audit_action: &'static str,
    event_type: &'static str,
}

#[derive(Debug, FromRow)]
struct SeasonRow {
    id: Uuid,
    name: String,
}

impl SeasonRow {
    fn into_snapshot(self) -> BotSeasonSnapshot {
        BotSeasonSnapshot {
            id: SeasonId::from_uuid(self.id),
            name: self.name,
        }
    }
}

#[derive(Debug, FromRow)]
struct RecruitmentLockRow {
    season_id: Uuid,
    status: RecruitmentStatus,
    target_count: i16,
    best_of: i16,
    hard_fearless: bool,
    revision: i64,
    created_by: UserId,
}

#[derive(Debug, FromRow)]
struct AuctionRecruitmentLockRow {
    season_id: Uuid,
    format: String,
    status: RecruitmentStatus,
    target_count: i16,
    semifinal_best_of: Option<i16>,
    final_best_of: i16,
    hard_fearless: bool,
    revision: i64,
    created_by: UserId,
}

#[derive(Debug, FromRow)]
struct AuctionRecruitmentProjectionRow {
    display_number: i64,
    title: String,
    format: String,
    target_count: i16,
    semifinal_best_of: Option<i16>,
    final_best_of: i16,
    hard_fearless: bool,
    status: RecruitmentStatus,
    revision: i64,
    channel_id: Option<DiscordChannelId>,
    message_id: Option<DiscordMessageId>,
    created_by: UserId,
    updated_at: OffsetDateTime,
    joined_participant_count: i64,
}

#[derive(Debug, FromRow)]
struct AuctionRecruitmentParticipantRow {
    id: UserId,
    display_name: String,
    declared_lane: Option<Lane>,
}

#[derive(Debug, FromRow)]
struct AuctionSetupDraftRow {
    revision: i64,
}

#[derive(Debug, FromRow)]
struct AuctionSetupTeamRow {
    team_index: i16,
    team_name: String,
    captain_user_id: Option<UserId>,
    initial_points: i32,
}

#[derive(Debug, FromRow)]
struct AuctionRecruitmentParticipantLockRow {
    user_id: UserId,
    declared_lane: Option<Lane>,
    mmr_at_join: Option<f64>,
    snapshot_origin: String,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentDraftRow {
    id: Uuid,
    season_id: Uuid,
    source_recruitment_id: Uuid,
    display_number: i64,
    title: String,
    format: String,
    semifinal_best_of: Option<i16>,
    final_best_of: i16,
    hard_fearless: bool,
    status: AuctionTournamentStatus,
    revision: i64,
    bracket_locked_at: Option<OffsetDateTime>,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentLockRow {
    display_number: i64,
    title: String,
    format: String,
    hard_fearless: bool,
    status: AuctionTournamentStatus,
    revision: i64,
    bracket_locked_at: Option<OffsetDateTime>,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentCancelLockRow {
    source_recruitment_id: AuctionRecruitmentId,
    status: AuctionTournamentStatus,
    revision: i64,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentCancelPreviewRow {
    id: Uuid,
    display_number: i64,
    title: String,
    status: AuctionTournamentStatus,
    revision: i64,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentBracketLockRow {
    season_id: Uuid,
    format: String,
    semifinal_best_of: Option<i16>,
    final_best_of: i16,
    hard_fearless: bool,
    status: AuctionTournamentStatus,
    revision: i64,
    bracket_locked_at: Option<OffsetDateTime>,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentBracketRow {
    id: Uuid,
    display_number: i64,
    title: String,
    format: String,
    semifinal_best_of: Option<i16>,
    final_best_of: i16,
    hard_fearless: bool,
    status: AuctionTournamentStatus,
    revision: i64,
    bracket_locked_at: Option<OffsetDateTime>,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentBracketSlotRow {
    slot_key: String,
    team_id: Option<Uuid>,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentBracketMatchRow {
    id: Uuid,
    match_key: String,
    best_of: i16,
    hard_fearless: bool,
    left_team_id: Option<Uuid>,
    right_team_id: Option<Uuid>,
    status: String,
    revision: i64,
    winner_team_id: Option<Uuid>,
    lineups_locked_at: Option<OffsetDateTime>,
    left_score: i64,
    right_score: i64,
}

#[derive(Debug, FromRow)]
struct AuctionMatchSnapshotRow {
    id: Uuid,
    tournament_id: Uuid,
    match_key: String,
    best_of: i16,
    hard_fearless: bool,
    status: String,
    revision: i64,
    left_team_id: Option<Uuid>,
    right_team_id: Option<Uuid>,
    winner_team_id: Option<Uuid>,
    lineups_locked_at: Option<OffsetDateTime>,
    lineups_locked_by: Option<UserId>,
    left_score: i64,
    right_score: i64,
}

#[derive(Debug, FromRow)]
struct AuctionFinalMatchRevertRow {
    id: Uuid,
    status: String,
    left_team_id: Option<Uuid>,
    right_team_id: Option<Uuid>,
    lineups_locked_at: Option<OffsetDateTime>,
}

#[derive(Debug, FromRow)]
struct AuctionMatchScoreRow {
    left_score: i64,
    right_score: i64,
}

#[derive(Debug, FromRow)]
struct AuctionConfirmedGameRow {
    id: Uuid,
    source_input_id: Uuid,
    set_number: i16,
    side_preset: String,
    winning_side: String,
    patch_version: Option<String>,
}

#[derive(Debug, FromRow)]
struct AuctionMatchLockRow {
    tournament_id: Uuid,
    season_id: Uuid,
    match_key: String,
    best_of: i16,
    hard_fearless: bool,
    status: String,
    revision: i64,
    left_team_id: Option<Uuid>,
    right_team_id: Option<Uuid>,
    winner_team_id: Option<Uuid>,
    lineups_locked_at: Option<OffsetDateTime>,
    lineups_locked_by: Option<UserId>,
}

#[derive(Debug, FromRow)]
struct AuctionMatchByKeyLockRow {
    id: Uuid,
    tournament_id: Uuid,
    season_id: Uuid,
    match_key: String,
    best_of: i16,
    hard_fearless: bool,
    status: String,
    revision: i64,
    left_team_id: Option<Uuid>,
    right_team_id: Option<Uuid>,
    winner_team_id: Option<Uuid>,
    lineups_locked_at: Option<OffsetDateTime>,
    lineups_locked_by: Option<UserId>,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentCompletionLockRow {
    format: String,
    status: AuctionTournamentStatus,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentResultNotificationRow {
    display_number: i64,
    title: String,
    format: String,
    semifinal_best_of: Option<i16>,
    final_best_of: i16,
    hard_fearless: bool,
    champion_team_id: Option<Uuid>,
    ended_at: Option<OffsetDateTime>,
    champion_team_name: String,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentResultOutboxLockRow {
    id: Uuid,
    channel_id: DiscordChannelId,
    discord_message_id: Option<DiscordMessageId>,
    delivered_at: Option<OffsetDateTime>,
    failed_at: Option<OffsetDateTime>,
    failure_code: Option<String>,
    attempts: i32,
    delivery_in_progress: bool,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentResultNotificationMatchRow {
    match_key: String,
    best_of: i16,
    left_team_name: String,
    right_team_name: String,
    winner_team_name: String,
    left_score: i64,
    right_score: i64,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentResultRow {
    id: Uuid,
    display_number: i64,
    title: String,
    format: String,
    semifinal_best_of: Option<i16>,
    final_best_of: i16,
    hard_fearless: bool,
    status: AuctionTournamentStatus,
    revision: i64,
    champion_team_id: Option<Uuid>,
    bracket_locked_at: Option<OffsetDateTime>,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentResultMemberRow {
    team_id: Uuid,
    team_member_id: Uuid,
    user_id: UserId,
    display_name: String,
    declared_lane: Option<Lane>,
    acquired_via: String,
    acquisition_points: Option<i32>,
    roster_slot: i16,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentResultMatchRow {
    id: Uuid,
    match_key: String,
    best_of: i16,
    hard_fearless: bool,
    status: String,
    left_team_id: Option<Uuid>,
    right_team_id: Option<Uuid>,
    winner_team_id: Option<Uuid>,
    lineups_locked_at: Option<OffsetDateTime>,
    left_score: i64,
    right_score: i64,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentResultLineupRow {
    match_id: Uuid,
    team_id: Uuid,
    lane: Lane,
    team_member_id: Uuid,
    user_id: UserId,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentResultPlayerRow {
    user_id: UserId,
    display_name: String,
    team_id: Uuid,
    games: i64,
    wins: i64,
    mmr_delta: Option<f64>,
}

#[derive(Debug, FromRow)]
struct AuctionMatchLineupRow {
    team_id: Uuid,
    lane: Lane,
    team_member_id: Uuid,
    user_id: UserId,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentTeamRow {
    id: Uuid,
    team_index: i16,
    team_name: String,
    initial_points: i32,
    current_points: i32,
    point_revision: i64,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentTeamMemberRow {
    team_id: Uuid,
    id: Uuid,
    user_id: UserId,
    display_name: String,
    declared_lane: Option<Lane>,
    mmr_at_acquisition: Option<f64>,
    snapshot_origin: String,
    acquired_via: String,
    roster_slot: i16,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentCandidateRow {
    user_id: UserId,
    display_name: String,
    declared_lane: Option<Lane>,
    mmr_at_start: Option<f64>,
    snapshot_origin: String,
    queue_position: i64,
    state: String,
    draw_count: i32,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentCurrentTargetProfileRow {
    avatar_hash: Option<String>,
    main_lane: Option<Lane>,
    profile_icon_id: Option<i32>,
    riot_snapshot: Option<Json<Value>>,
    riot_refreshed_at: Option<OffsetDateTime>,
    solo_rank_tier: Option<String>,
    solo_rank_division: Option<String>,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentCurrentTargetLaneMmrRow {
    lane: Lane,
    mmr: f64,
    games_played: i32,
    wins: i32,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentCurrentTargetChampionRow {
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentCurrentTargetChampionRecordRow {
    champion_id: String,
    games: i64,
    wins: i64,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentCurrentTargetRecentGameRow {
    id: Uuid,
    played_at: OffsetDateTime,
    lane: Lane,
    champion_id: Option<String>,
    won: bool,
}

#[derive(Debug, FromRow)]
struct AuctionTournamentCandidateLockRow {
    user_id: UserId,
    declared_lane: Option<Lane>,
    mmr_at_start: Option<f64>,
    snapshot_origin: String,
    queue_position: i64,
    state: String,
}

#[derive(Debug, FromRow)]
struct AuctionLotLockRow {
    id: Uuid,
    draw_sequence: i64,
    target_user_id: UserId,
    status: String,
}

#[derive(Debug, FromRow)]
struct AuctionActionLockRow {
    id: Uuid,
    sequence: i64,
    kind: String,
    target_user_id: Option<UserId>,
    team_id: Option<Uuid>,
    lot_id: Option<Uuid>,
    points: Option<i32>,
    reverses_action_id: Option<Uuid>,
}

#[derive(Debug, FromRow)]
struct AuctionTeamMemberLockRow {
    id: Uuid,
    team_id: Uuid,
    user_id: UserId,
    roster_slot: i16,
    acquired_via: String,
}

#[derive(Debug, FromRow)]
struct AuctionPointLedgerLockRow {
    id: Uuid,
    team_id: Uuid,
    point_revision: i64,
    reason: String,
    source_action_id: Option<Uuid>,
    points_before: i32,
    points_after: i32,
}

#[derive(Debug, FromRow)]
struct SeriesLockRow {
    source_recruitment_id: RecruitmentId,
    best_of: i16,
    hard_fearless: bool,
    status: SeriesStatus,
    revision: i64,
}

#[derive(Debug, FromRow)]
struct RecruitmentProjectionRow {
    id: Uuid,
    display_number: i64,
    title: String,
    status: RecruitmentStatus,
    target_count: i16,
    best_of: i16,
    hard_fearless: bool,
    revision: i64,
    channel_id: Option<DiscordChannelId>,
    message_id: Option<DiscordMessageId>,
    created_by: UserId,
    updated_at: OffsetDateTime,
}

impl RecruitmentProjectionRow {
    fn into_snapshot(self) -> RecruitmentSummarySnapshot {
        RecruitmentSummarySnapshot {
            id: RecruitmentId::from_uuid(self.id),
            display_number: self.display_number,
            title: self.title,
            status: self.status,
            target_count: self.target_count,
            best_of: self.best_of,
            hard_fearless: self.hard_fearless,
            revision: self.revision,
            channel_id: self.channel_id,
            message_id: self.message_id,
            created_by_id: self.created_by,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct RecruitmentParticipantRow {
    id: UserId,
    display_name: String,
}

#[derive(Debug, FromRow)]
struct RecruitmentLaneRow {
    user_id: UserId,
    lane: Lane,
}

#[derive(Debug, FromRow)]
struct EntryDraftRecruitmentRow {
    id: Uuid,
    season_id: Uuid,
    display_number: i64,
    title: String,
    status: RecruitmentStatus,
    target_count: i16,
    best_of: i16,
    hard_fearless: bool,
    revision: i64,
    updated_at: OffsetDateTime,
    converted_at: Option<OffsetDateTime>,
    created_by: UserId,
    draft_revision: i64,
    draft_updated_at: OffsetDateTime,
    draft_updated_by: Option<UserId>,
    draft_updated_by_display_name: Option<String>,
}

#[derive(Debug, FromRow)]
struct EntryDraftParticipantRow {
    id: UserId,
    display_name: String,
    main_lane: Option<Lane>,
    mmr: f64,
    games: i32,
    wins: i32,
    solo_rank_tier: Option<String>,
    solo_rank_division: Option<String>,
    profile_icon_id: Option<i32>,
}

#[derive(Debug, FromRow)]
struct EntryDraftHeadToHeadRow {
    lane: Lane,
    user_a_id: UserId,
    user_b_id: UserId,
    plays: i64,
    user_a_wins: i64,
    user_b_wins: i64,
}

fn compact_riot_tier(tier: Option<&str>, division: Option<&str>) -> Option<String> {
    let prefix = match tier?.trim().to_ascii_uppercase().as_str() {
        "IRON" => "I",
        "BRONZE" => "B",
        "SILVER" => "S",
        "GOLD" => "G",
        "PLATINUM" => "P",
        "EMERALD" => "E",
        "DIAMOND" => "D",
        "MASTER" => "M",
        "GRANDMASTER" => "GM",
        "CHALLENGER" => "C",
        _ => return None,
    };
    let suffix = match division.map(str::trim).filter(|value| !value.is_empty()) {
        None => "",
        Some("I") => "1",
        Some("II") => "2",
        Some("III") => "3",
        Some("IV") => "4",
        Some(_) => return None,
    };

    Some(format!("{prefix}{suffix}"))
}

#[derive(Debug, FromRow)]
struct EntryDraftChampionRow {
    user_id: UserId,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanSeriesRow {
    id: Uuid,
    display_number: i64,
    title: String,
    best_of: i16,
    hard_fearless: bool,
    status: SeriesStatus,
    revision: i64,
    source_recruitment_id: Uuid,
    updated_at: OffsetDateTime,
}

#[derive(Debug, FromRow)]
struct SeriesResultSeriesRow {
    id: Uuid,
    display_number: i64,
    title: String,
    best_of: i16,
    hard_fearless: bool,
    status: SeriesStatus,
    revision: i64,
    source_recruitment_id: Uuid,
    winning_team: Option<String>,
    terminal_reason: Option<String>,
    started_at: OffsetDateTime,
    ended_at: Option<OffsetDateTime>,
}

#[derive(Debug, FromRow)]
struct SeriesResultNotificationSeriesRow {
    id: Uuid,
    display_number: i64,
    title: String,
    best_of: i16,
    hard_fearless: bool,
    winning_team: Option<String>,
    terminal_reason: Option<String>,
}

#[derive(Debug, FromRow)]
struct SeriesResultGameRow {
    id: Uuid,
    source_input_id: Uuid,
    set_number: i16,
    side_preset: String,
    winning_side: String,
    patch_version: Option<String>,
    duration_seconds: Option<i32>,
}

#[derive(Debug, FromRow)]
struct SeriesResultNotificationGameRow {
    id: Uuid,
    set_number: i16,
    side_preset: String,
    winning_side: String,
    patch_version: Option<String>,
}

#[derive(Debug, FromRow)]
struct SeriesResultNotificationRosterRow {
    team: String,
    id: UserId,
    lane: Lane,
    display_name: String,
}

#[derive(Debug, FromRow)]
struct SeriesResultNotificationGameParticipantRow {
    game_id: Uuid,
    team: String,
    id: UserId,
    lane: Lane,
    champion_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct SeriesResultNotificationGameBanRow {
    game_id: Uuid,
    side: String,
    slot: i16,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct SeriesResultMmrChangeRow {
    user_id: UserId,
    lane: Lane,
    delta: f64,
    mmr_before: f64,
    mmr_after: f64,
}

#[derive(Debug, FromRow)]
struct EntryConfirmationParticipantRow {
    team: String,
    id: UserId,
    lane: Lane,
    display_name: String,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanParticipantRow {
    team: String,
    id: UserId,
    lane: Lane,
    mmr: Option<f64>,
    display_name: String,
    solo_rank_tier: Option<String>,
    solo_rank_division: Option<String>,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanChampionRow {
    user_id: UserId,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanGameInputRow {
    id: Uuid,
    set_number: i16,
    side_preset: String,
    side_selected: bool,
    winning_side: Option<String>,
    patch_version: Option<String>,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanPickRow {
    user_id: UserId,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanBanRow {
    side: String,
    slot: i16,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct SeriesRosterRow {
    user_id: UserId,
    team: String,
    lane: Lane,
}

#[derive(Debug, FromRow)]
struct ConfirmedSeriesGameRow {
    id: Uuid,
    season_id: Uuid,
    source_input_id: Uuid,
    game_number: i16,
}

#[derive(Debug, FromRow)]
struct GameResultLedgerRow {
    id: Uuid,
    user_id: UserId,
    lane: Lane,
    lane_revision: i64,
    opponent_user_id: UserId,
    mmr_before: f64,
    mmr_after: f64,
    delta: f64,
    games_played_delta: i16,
    wins_delta: i16,
}

#[derive(Debug)]
struct VoidedSeriesGame {
    game_id: GameId,
    set_number: i16,
}

#[derive(Clone, Copy)]
enum ConfirmedGameParent {
    Series(SeriesId),
    AuctionMatch(AuctionMatchId),
    AuctionTournament(AuctionTournamentId),
}

impl ConfirmedGameParent {
    fn mmr_history_error(self) -> AttemptError {
        match self {
            Self::Series(_) => AttemptError::SeriesGameMmrHistoryAdvanced,
            Self::AuctionMatch(_) => AttemptError::AuctionMatchGameMmrHistoryAdvanced,
            Self::AuctionTournament(_) => AttemptError::AuctionTournamentCancelMmrHistoryAdvanced,
        }
    }

    fn reversal_metadata(self) -> (&'static str, &'static str) {
        match self {
            Self::Series(_) | Self::AuctionMatch(_) => ("USER", "OPERATOR_REVERT"),
            Self::AuctionTournament(_) => ("BOT", "TOURNAMENT_CANCELLED"),
        }
    }
}

#[derive(Debug, FromRow)]
struct EntryDraftAssignmentRow {
    participant_id: UserId,
    team: String,
    lane: Lane,
}

#[derive(Debug)]
struct SeriesEntryAssignment {
    user_id: UserId,
    team: String,
    lane: Lane,
}

#[derive(Debug, FromRow)]
struct EntryDraftAuctionConversionParticipantRow {
    user_id: UserId,
    declared_lane: Option<Lane>,
}

#[derive(Debug)]
struct AuctionConversionParticipant {
    user_id: UserId,
    declared_lane: Option<Lane>,
    mmr_at_join: Option<f64>,
    snapshot_origin: &'static str,
}

impl EntryDraftAssignmentRow {
    fn into_snapshot(self) -> Result<EntryDraftAssignmentSnapshot, AttemptError> {
        if !matches!(self.team.as_str(), "TEAM_1" | "TEAM_2") {
            return Err(AttemptError::InvariantViolation);
        }
        Ok(EntryDraftAssignmentSnapshot {
            participant_id: self.participant_id,
            team: self.team,
            lane: self.lane,
        })
    }
}

#[derive(Debug, Error)]
enum AttemptError {
    #[error("PostgreSQL bot workflow operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("MMR repository operation failed")]
    Mmr(#[from] MmrRepositoryError),
    #[error("idempotency repository operation failed")]
    Idempotency(#[from] IdempotencyRepositoryError),
    #[error("notification outbox operation failed")]
    Notification(#[from] NotificationOutboxError),
    #[error("Riot account repository operation failed")]
    Riot(#[from] RiotRepositoryError),
    #[error("Discord identity conflicts with a non-Discord user")]
    IdentityConflict,
    #[error("bot workflow JSON serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("guild was not found")]
    GuildNotFound,
    #[error("guild is not active: {0}")]
    GuildInactive(GuildStatus),
    #[error("guild setup has not created a configuration")]
    GuildNotConfigured,
    #[error("guild setup was already completed")]
    GuildAlreadyConfigured,
    #[error("guild cannot be configured while its status is {0}")]
    GuildSetupUnavailable(GuildStatus),
    #[error("the actor does not hold the configured operator role")]
    OperatorRequired,
    #[error("the guild member is inactive")]
    MemberInactive,
    #[error("the caller is not an active member of this guild")]
    MembershipRequired,
    #[error("the recruitment member was not found in this guild")]
    RecruitmentMemberNotFound,
    #[error("the recruitment member is inactive")]
    RecruitmentMemberInactive,
    #[error("the guild has no active season")]
    NoActiveSeason,
    #[error("the guild has no configured recruitment channel")]
    RecruitmentChannelNotConfigured,
    #[error("the guild recruitment counter is missing")]
    MissingRecruitmentCounter,
    #[error("the guild series counter is missing")]
    MissingSeriesCounter,
    #[error("the guild auction recruitment counter is missing")]
    MissingAuctionRecruitmentCounter,
    #[error("the guild auction tournament counter is missing")]
    MissingAuctionTournamentCounter,
    #[error("recruitment was not found in the tenant")]
    RecruitmentNotFound,
    #[error("auction recruitment was not found in the tenant")]
    AuctionRecruitmentNotFound,
    #[error("auction tournament was not found in the tenant")]
    AuctionTournamentNotFound,
    #[error("recruitment must be OPEN, but is {0}")]
    RecruitmentNotOpen(RecruitmentStatus),
    #[error(
        "participant confirmation DMs can only be requested while recruitment is CLOSED, but is {0}"
    )]
    ParticipantConfirmationDmUnavailable(RecruitmentStatus),
    #[error("recruitment message cannot be bound while status is {0}")]
    RecruitmentNotBindable(RecruitmentStatus),
    #[error("recruitment is full")]
    RecruitmentFull,
    #[error("auction recruitment requires a declared lane")]
    AuctionDeclaredLaneRequired,
    #[error("participant is already joined")]
    ParticipantAlreadyJoined,
    #[error("participant is not joined")]
    ParticipantNotJoined,
    #[error("preferred lanes are invalid")]
    InvalidPreferredLanes,
    #[error("entry draft root is missing")]
    MissingEntryDraft,
    #[error("entry draft can only be changed while recruitment is CLOSED, but is {0}")]
    EntryDraftNotEditable(RecruitmentStatus),
    #[error("entry draft revision mismatch: expected {expected}, actual {actual}")]
    EntryDraftRevisionConflict { expected: i64, actual: i64 },
    #[error("entry draft source recruitment must be CLOSED for auction conversion, but is {0}")]
    EntryDraftAuctionSourceNotReady(RecruitmentStatus),
    #[error("entry draft source recruitment already has an active series")]
    EntryDraftAlreadyConvertedToSeries,
    #[error("auction setup draft root is missing")]
    MissingAuctionSetupDraft,
    #[error("auction setup can only be changed while recruitment is CLOSED, but is {0}")]
    AuctionSetupDraftNotEditable(RecruitmentStatus),
    #[error("auction setup draft revision mismatch: expected {expected}, actual {actual}")]
    AuctionSetupDraftRevisionConflict { expected: i64, actual: i64 },
    #[error("auction setup requires {expected} ordered teams, received {actual}")]
    AuctionSetupTeamCountInvalid { expected: i16, actual: usize },
    #[error("auction setup team index mismatch: expected {expected}, actual {actual}")]
    AuctionSetupTeamIndexInvalid { expected: i16, actual: i16 },
    #[error("auction setup team name is invalid")]
    AuctionSetupTeamNameInvalid,
    #[error("auction setup initial points must be positive")]
    AuctionSetupInitialPointsInvalid,
    #[error("auction setup captains must be unique")]
    AuctionSetupDuplicateCaptain,
    #[error("auction setup captain is not a joined recruitment participant")]
    AuctionSetupCaptainNotJoined,
    #[error("auction setup is incomplete: configured {configured}, required {required}")]
    AuctionSetupDraftIncomplete { configured: i16, required: i16 },
    #[error("auction tournament source recruitment must be CLOSED, but is {0}")]
    AuctionTournamentSourceNotReady(RecruitmentStatus),
    #[error("an active auction tournament already exists for this recruitment")]
    AuctionTournamentAlreadyExists,
    #[error("auction tournament revision mismatch: expected {expected}, actual {actual}")]
    AuctionTournamentRevisionConflict { expected: i64, actual: i64 },
    #[error("auction tournament draft cannot be changed while status is {0}")]
    AuctionTournamentDraftNotEditable(AuctionTournamentStatus),
    #[error("auction tournament draft is locked by bracket or match artifacts")]
    AuctionTournamentDraftLocked,
    #[error("auction draft candidate was not found")]
    AuctionDraftCandidateNotFound,
    #[error("auction draft team was not found")]
    AuctionDraftTeamNotFound,
    #[error("auction draft unsold lot was not found")]
    AuctionDraftUnsoldLotNotFound,
    #[error("auction draft requires an active target")]
    AuctionDraftCurrentTargetRequired,
    #[error("auction draft already has an active target")]
    AuctionDraftCurrentTargetExists,
    #[error("auction draft target does not match the active lot")]
    AuctionDraftCurrentTargetMismatch,
    #[error("auction draft candidate is not in the required state")]
    AuctionDraftCandidateStateMismatch,
    #[error("auction draft has no queued candidate")]
    AuctionDraftNoQueuedCandidate,
    #[error("auction draft team has no free roster slot")]
    AuctionDraftTeamFull,
    #[error(
        "auction bid points are invalid: current={current_points}, submitted={submitted_points}"
    )]
    AuctionDraftBidPointsInvalid {
        current_points: i32,
        submitted_points: i32,
    },
    #[error("auction draft has no action that can be reverted")]
    AuctionDraftNoRevertableAction,
    #[error("auction draft action can no longer be reverted safely")]
    AuctionDraftRevertUnavailable,
    #[error("auction tournament bracket cannot be changed while status is {0}")]
    AuctionTournamentBracketNotEditable(AuctionTournamentStatus),
    #[error("auction tournament bracket is unavailable while status is {0}")]
    AuctionTournamentBracketUnavailable(AuctionTournamentStatus),
    #[error("auction tournament bracket is already locked or has match artifacts")]
    AuctionTournamentBracketLocked,
    #[error("auction tournament bracket recovery is unavailable because match artifacts exist")]
    AuctionTournamentBracketRecoveryUnavailable,
    #[error("auction tournament bracket slot is unavailable for this format")]
    AuctionTournamentBracketSlotUnavailable,
    #[error("auction tournament bracket team roster is incomplete")]
    AuctionTournamentBracketRosterIncomplete,
    #[error("auction tournament bracket slots are incomplete")]
    AuctionTournamentBracketSlotsIncomplete,
    #[error("auction match was not found in the tenant")]
    AuctionMatchNotFound,
    #[error("auction match is waiting for both bracket teams")]
    AuctionMatchTeamsPending,
    #[error("auction match tournament is not in an active bracket state")]
    AuctionMatchTournamentNotReady,
    #[error("auction match revision mismatch: expected {expected}, actual {actual}")]
    AuctionMatchRevisionConflict { expected: i64, actual: i64 },
    #[error("auction match lineup cannot be changed while status is {0}")]
    AuctionMatchLineupNotEditable(String),
    #[error("auction match lineup is already locked")]
    AuctionMatchLineupLocked,
    #[error("auction match lineup cannot be unlocked while status is {0}")]
    AuctionMatchLineupUnlockUnavailable(String),
    #[error("auction match lineup cannot be unlocked after a game input exists")]
    AuctionMatchLineupResultArtifactsPresent,
    #[error("auction match lineup team is not one of this match's teams")]
    AuctionMatchLineupTeamNotFound,
    #[error("auction match lineup player is not in the active team roster")]
    AuctionMatchLineupMemberNotFound,
    #[error("auction match lineup player is already assigned to another lane")]
    AuctionMatchLineupPlayerAssigned,
    #[error("auction match lineup must contain both teams' five lanes")]
    AuctionMatchLineupIncomplete,
    #[error(
        "both 20-person auction semifinal lineups must be locked before a game result is entered"
    )]
    AuctionMatchSemifinalLineupsIncomplete,
    #[error("auction match result cannot be changed while status is {0}")]
    AuctionMatchResultNotEditable(String),
    #[error("auction match game set mismatch: expected {expected}, actual {actual}")]
    AuctionMatchSetNotCurrent { expected: i16, actual: i16 },
    #[error(
        "the same champion appears more than once in the auction match game input: {champion_id}"
    )]
    AuctionMatchGameChampionDuplicate { champion_id: String },
    #[error("a hard fearless champion was already picked in this auction match: {champion_id}")]
    AuctionMatchHardFearlessChampionUsed { champion_id: String },
    #[error("auction match has no confirmed game to revert")]
    AuctionMatchNoConfirmedGames,
    #[error("a later MMR change prevents this auction match game from being reversed safely")]
    AuctionMatchGameMmrHistoryAdvanced,
    #[error("the downstream final already has lineup or game history")]
    AuctionMatchDownstreamWorkExists,
    #[error("auction tournament result is unavailable while status is {0}")]
    AuctionTournamentResultUnavailable(AuctionTournamentStatus),
    #[error("auction tournament result delivery is currently leased by the bot")]
    AuctionTournamentResultDeliveryInProgress,
    #[error("auction tournament result delivery state is uncertain")]
    AuctionTournamentResultDeliveryUncertain,
    #[error("auction tournament cannot be cancelled while status is {0}")]
    AuctionTournamentCancelUnavailable(AuctionTournamentStatus),
    #[error("a later MMR change prevents this auction tournament from being cancelled safely")]
    AuctionTournamentCancelMmrHistoryAdvanced,
    #[error("recruitment revision mismatch: expected {expected}, actual {actual}")]
    RecruitmentRevisionConflict { expected: i64, actual: i64 },
    #[error("series source recruitment must be CLOSED, but is {0}")]
    SeriesSourceNotReady(RecruitmentStatus),
    #[error("series was not found in the tenant")]
    SeriesNotFound,
    #[error("series must be IN_PROGRESS, but is {0}")]
    SeriesNotInProgress(SeriesStatus),
    #[error("series revision mismatch: expected {expected}, actual {actual}")]
    SeriesRevisionConflict { expected: i64, actual: i64 },
    #[error("series source recruitment must be CONVERTED, but is {0}")]
    SeriesSourceNotConverted(RecruitmentStatus),
    #[error("series has no current draft set")]
    SeriesNoCurrentSet,
    #[error("series game set mismatch: expected {expected}, actual {actual}")]
    SeriesSetNotCurrent { expected: i16, actual: i16 },
    #[error("series game side must be selected before pick-ban input")]
    SeriesGameSideRequired,
    #[error("series game side mismatch: expected {expected}, actual {actual}")]
    SeriesGameSideMismatch { expected: String, actual: String },
    #[error("series game result input is invalid")]
    InvalidGameResult,
    #[error("the same champion appears more than once in the game input: {champion_id}")]
    SeriesGameChampionDuplicate { champion_id: String },
    #[error("a hard fearless champion was already picked in this series: {champion_id}")]
    SeriesHardFearlessChampionUsed { champion_id: String },
    #[error("pick-ban player is not in the frozen series roster")]
    SeriesPickBanPlayerNotInRoster,
    #[error("series has no confirmed game to revert")]
    SeriesNoConfirmedGames,
    #[error(
        "series early completion requires BO3 or BO5 with at least one confirmed game: best_of={best_of}, confirmed_game_count={confirmed_game_count}"
    )]
    SeriesEarlyCompletionUnavailable {
        best_of: i16,
        confirmed_game_count: i64,
    },
    #[error(
        "series early draw requires a tied score: team_1_score={team_1_score}, team_2_score={team_2_score}"
    )]
    SeriesEarlyDrawRequiresTiedScore {
        team_1_score: i64,
        team_2_score: i64,
    },
    #[error("a later MMR change prevents this game from being reversed safely")]
    SeriesGameMmrHistoryAdvanced,
    #[error("series result is unavailable while status is {0}")]
    SeriesResultUnavailable(SeriesStatus),
    #[error("entry draft must assign all joined participants before series creation")]
    EntryDraftIncomplete {
        joined: i64,
        assigned: i64,
        target: i16,
    },
    #[error("entry draft slot is invalid")]
    InvalidEntrySlot,
    #[error("closing recruitment requires {target} joined participants, found {joined}")]
    CloseRequiresFullRoster { joined: i64, target: i16 },
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict,
    #[error("the same idempotent command is still in progress")]
    IdempotencyInProgress,
    #[error("stored idempotent response is corrupt")]
    CorruptIdempotentResponse,
    #[error("idempotency completion row was not in progress")]
    IdempotencyCompletionLost,
    #[error("bot workflow storage invariant was violated")]
    InvariantViolation,
}

impl AttemptError {
    fn into_public(self) -> BotWorkflowError {
        match self {
            Self::Sql(error) => BotWorkflowError::Database(error),
            Self::Mmr(MmrRepositoryError::Sql(error)) => BotWorkflowError::Database(error),
            Self::Mmr(MmrRepositoryError::MissingInitializedRow) => {
                BotWorkflowError::InvariantViolation
            }
            Self::Idempotency(IdempotencyRepositoryError::Sql(error)) => {
                BotWorkflowError::Database(error)
            }
            Self::Idempotency(_) => BotWorkflowError::InvariantViolation,
            Self::Notification(NotificationOutboxError::Sql(error)) => {
                BotWorkflowError::Database(error)
            }
            Self::Notification(_) => BotWorkflowError::InvariantViolation,
            Self::Riot(RiotRepositoryError::Sql(error)) => BotWorkflowError::Database(error),
            Self::Riot(RiotRepositoryError::AlreadyLinked) => {
                BotWorkflowError::RiotAccountAlreadyLinked
            }
            Self::Riot(RiotRepositoryError::IdentityConflict) => {
                BotWorkflowError::RiotIdentityConflict
            }
            Self::Riot(
                RiotRepositoryError::UserNotFound
                | RiotRepositoryError::NotFound
                | RiotRepositoryError::InvalidCanonicalAccount
                | RiotRepositoryError::InvariantViolation,
            ) => BotWorkflowError::InvariantViolation,
            Self::Serialization(error) => BotWorkflowError::Serialization(error),
            Self::GuildNotFound => BotWorkflowError::GuildNotFound,
            Self::GuildInactive(status) => BotWorkflowError::GuildInactive(status),
            Self::GuildNotConfigured => BotWorkflowError::GuildNotConfigured,
            Self::GuildAlreadyConfigured => BotWorkflowError::GuildAlreadyConfigured,
            Self::GuildSetupUnavailable(status) => BotWorkflowError::GuildSetupUnavailable(status),
            Self::OperatorRequired => BotWorkflowError::OperatorRequired,
            Self::MemberInactive => BotWorkflowError::MemberInactive,
            Self::MembershipRequired => BotWorkflowError::MembershipRequired,
            Self::RecruitmentMemberNotFound => BotWorkflowError::RecruitmentMemberNotFound,
            Self::RecruitmentMemberInactive => BotWorkflowError::RecruitmentMemberInactive,
            Self::NoActiveSeason => BotWorkflowError::NoActiveSeason,
            Self::RecruitmentChannelNotConfigured => {
                BotWorkflowError::RecruitmentChannelNotConfigured
            }
            Self::MissingRecruitmentCounter
            | Self::MissingSeriesCounter
            | Self::MissingAuctionRecruitmentCounter
            | Self::MissingAuctionTournamentCounter => BotWorkflowError::InvariantViolation,
            Self::RecruitmentNotFound => BotWorkflowError::RecruitmentNotFound,
            Self::AuctionRecruitmentNotFound => BotWorkflowError::RecruitmentNotFound,
            Self::AuctionTournamentNotFound => BotWorkflowError::AuctionTournamentNotFound,
            Self::RecruitmentNotOpen(status) => BotWorkflowError::RecruitmentNotOpen(status),
            Self::ParticipantConfirmationDmUnavailable(status) => {
                BotWorkflowError::ParticipantConfirmationDmUnavailable(status)
            }
            Self::RecruitmentNotBindable(status) => {
                BotWorkflowError::RecruitmentNotBindable(status)
            }
            Self::RecruitmentFull => BotWorkflowError::RecruitmentFull,
            Self::AuctionDeclaredLaneRequired => BotWorkflowError::AuctionDeclaredLaneRequired,
            Self::ParticipantAlreadyJoined => BotWorkflowError::ParticipantAlreadyJoined,
            Self::ParticipantNotJoined => BotWorkflowError::ParticipantNotJoined,
            Self::InvalidPreferredLanes => BotWorkflowError::InvalidPreferredLanes,
            Self::MissingEntryDraft => BotWorkflowError::InvariantViolation,
            Self::EntryDraftNotEditable(status) => BotWorkflowError::EntryDraftNotEditable(status),
            Self::EntryDraftRevisionConflict { expected, actual } => {
                BotWorkflowError::EntryDraftRevisionConflict { expected, actual }
            }
            Self::EntryDraftAuctionSourceNotReady(status) => {
                BotWorkflowError::EntryDraftAuctionSourceNotReady(status)
            }
            Self::EntryDraftAlreadyConvertedToSeries => {
                BotWorkflowError::EntryDraftAlreadyConvertedToSeries
            }
            Self::MissingAuctionSetupDraft => BotWorkflowError::InvariantViolation,
            Self::AuctionSetupDraftNotEditable(status) => {
                BotWorkflowError::AuctionSetupDraftNotEditable(status)
            }
            Self::AuctionSetupDraftRevisionConflict { expected, actual } => {
                BotWorkflowError::AuctionSetupDraftRevisionConflict { expected, actual }
            }
            Self::AuctionSetupTeamCountInvalid { expected, actual } => {
                BotWorkflowError::AuctionSetupTeamCountInvalid { expected, actual }
            }
            Self::AuctionSetupTeamIndexInvalid { expected, actual } => {
                BotWorkflowError::AuctionSetupTeamIndexInvalid { expected, actual }
            }
            Self::AuctionSetupTeamNameInvalid => BotWorkflowError::AuctionSetupTeamNameInvalid,
            Self::AuctionSetupInitialPointsInvalid => {
                BotWorkflowError::AuctionSetupInitialPointsInvalid
            }
            Self::AuctionSetupDuplicateCaptain => BotWorkflowError::AuctionSetupDuplicateCaptain,
            Self::AuctionSetupCaptainNotJoined => BotWorkflowError::AuctionSetupCaptainNotJoined,
            Self::AuctionSetupDraftIncomplete {
                configured,
                required,
            } => BotWorkflowError::AuctionSetupDraftIncomplete {
                configured,
                required,
            },
            Self::AuctionTournamentSourceNotReady(status) => {
                BotWorkflowError::AuctionTournamentSourceNotReady(status)
            }
            Self::AuctionTournamentAlreadyExists => {
                BotWorkflowError::AuctionTournamentAlreadyExists
            }
            Self::AuctionTournamentRevisionConflict { expected, actual } => {
                BotWorkflowError::AuctionTournamentRevisionConflict { expected, actual }
            }
            Self::AuctionTournamentDraftNotEditable(status) => {
                BotWorkflowError::AuctionTournamentDraftNotEditable(status)
            }
            Self::AuctionTournamentDraftLocked => BotWorkflowError::AuctionTournamentDraftLocked,
            Self::AuctionDraftCandidateNotFound => BotWorkflowError::AuctionDraftCandidateNotFound,
            Self::AuctionDraftTeamNotFound => BotWorkflowError::AuctionDraftTeamNotFound,
            Self::AuctionDraftUnsoldLotNotFound => BotWorkflowError::AuctionDraftUnsoldLotNotFound,
            Self::AuctionDraftCurrentTargetRequired => {
                BotWorkflowError::AuctionDraftCurrentTargetRequired
            }
            Self::AuctionDraftCurrentTargetExists => {
                BotWorkflowError::AuctionDraftCurrentTargetExists
            }
            Self::AuctionDraftCurrentTargetMismatch => {
                BotWorkflowError::AuctionDraftCurrentTargetMismatch
            }
            Self::AuctionDraftCandidateStateMismatch => {
                BotWorkflowError::AuctionDraftCandidateStateMismatch
            }
            Self::AuctionDraftNoQueuedCandidate => BotWorkflowError::AuctionDraftNoQueuedCandidate,
            Self::AuctionDraftTeamFull => BotWorkflowError::AuctionDraftTeamFull,
            Self::AuctionDraftBidPointsInvalid {
                current_points,
                submitted_points,
            } => BotWorkflowError::AuctionDraftBidPointsInvalid {
                current_points,
                submitted_points,
            },
            Self::AuctionDraftNoRevertableAction => {
                BotWorkflowError::AuctionDraftNoRevertableAction
            }
            Self::AuctionDraftRevertUnavailable => BotWorkflowError::AuctionDraftRevertUnavailable,
            Self::AuctionTournamentBracketNotEditable(status) => {
                BotWorkflowError::AuctionTournamentBracketNotEditable(status)
            }
            Self::AuctionTournamentBracketUnavailable(status) => {
                BotWorkflowError::AuctionTournamentBracketUnavailable(status)
            }
            Self::AuctionTournamentBracketLocked => {
                BotWorkflowError::AuctionTournamentBracketLocked
            }
            Self::AuctionTournamentBracketRecoveryUnavailable => {
                BotWorkflowError::AuctionTournamentBracketRecoveryUnavailable
            }
            Self::AuctionTournamentBracketSlotUnavailable => {
                BotWorkflowError::AuctionTournamentBracketSlotUnavailable
            }
            Self::AuctionTournamentBracketRosterIncomplete => {
                BotWorkflowError::AuctionTournamentBracketRosterIncomplete
            }
            Self::AuctionTournamentBracketSlotsIncomplete => {
                BotWorkflowError::AuctionTournamentBracketSlotsIncomplete
            }
            Self::AuctionMatchNotFound => BotWorkflowError::AuctionMatchNotFound,
            Self::AuctionMatchTeamsPending => BotWorkflowError::AuctionMatchTeamsPending,
            Self::AuctionMatchTournamentNotReady => {
                BotWorkflowError::AuctionMatchTournamentNotReady
            }
            Self::AuctionMatchRevisionConflict { expected, actual } => {
                BotWorkflowError::AuctionMatchRevisionConflict { expected, actual }
            }
            Self::AuctionMatchLineupNotEditable(status) => {
                BotWorkflowError::AuctionMatchLineupNotEditable(status)
            }
            Self::AuctionMatchLineupLocked => BotWorkflowError::AuctionMatchLineupLocked,
            Self::AuctionMatchLineupUnlockUnavailable(status) => {
                BotWorkflowError::AuctionMatchLineupUnlockUnavailable(status)
            }
            Self::AuctionMatchLineupResultArtifactsPresent => {
                BotWorkflowError::AuctionMatchLineupResultArtifactsPresent
            }
            Self::AuctionMatchLineupTeamNotFound => {
                BotWorkflowError::AuctionMatchLineupTeamNotFound
            }
            Self::AuctionMatchLineupMemberNotFound => {
                BotWorkflowError::AuctionMatchLineupMemberNotFound
            }
            Self::AuctionMatchLineupPlayerAssigned => {
                BotWorkflowError::AuctionMatchLineupPlayerAssigned
            }
            Self::AuctionMatchLineupIncomplete => BotWorkflowError::AuctionMatchLineupIncomplete,
            Self::AuctionMatchSemifinalLineupsIncomplete => {
                BotWorkflowError::AuctionMatchSemifinalLineupsIncomplete
            }
            Self::AuctionMatchResultNotEditable(status) => {
                BotWorkflowError::AuctionMatchResultNotEditable(status)
            }
            Self::AuctionMatchSetNotCurrent { expected, actual } => {
                BotWorkflowError::AuctionMatchSetNotCurrent { expected, actual }
            }
            Self::AuctionMatchGameChampionDuplicate { champion_id } => {
                BotWorkflowError::AuctionMatchGameChampionDuplicate { champion_id }
            }
            Self::AuctionMatchHardFearlessChampionUsed { champion_id } => {
                BotWorkflowError::AuctionMatchHardFearlessChampionUsed { champion_id }
            }
            Self::AuctionMatchNoConfirmedGames => BotWorkflowError::AuctionMatchNoConfirmedGames,
            Self::AuctionMatchGameMmrHistoryAdvanced => {
                BotWorkflowError::AuctionMatchGameMmrHistoryAdvanced
            }
            Self::AuctionMatchDownstreamWorkExists => {
                BotWorkflowError::AuctionMatchDownstreamWorkExists
            }
            Self::AuctionTournamentResultUnavailable(status) => {
                BotWorkflowError::AuctionTournamentResultUnavailable(status)
            }
            Self::AuctionTournamentResultDeliveryInProgress => {
                BotWorkflowError::AuctionTournamentResultDeliveryInProgress
            }
            Self::AuctionTournamentResultDeliveryUncertain => {
                BotWorkflowError::AuctionTournamentResultDeliveryUncertain
            }
            Self::AuctionTournamentCancelUnavailable(status) => {
                BotWorkflowError::AuctionTournamentCancelUnavailable(status)
            }
            Self::AuctionTournamentCancelMmrHistoryAdvanced => {
                BotWorkflowError::AuctionTournamentCancelMmrHistoryAdvanced
            }
            Self::RecruitmentRevisionConflict { expected, actual } => {
                BotWorkflowError::RecruitmentRevisionConflict { expected, actual }
            }
            Self::SeriesSourceNotReady(status) => BotWorkflowError::SeriesSourceNotReady(status),
            Self::SeriesNotFound => BotWorkflowError::SeriesNotFound,
            Self::SeriesNotInProgress(status) => BotWorkflowError::SeriesNotInProgress(status),
            Self::SeriesRevisionConflict { expected, actual } => {
                BotWorkflowError::SeriesRevisionConflict { expected, actual }
            }
            Self::SeriesSourceNotConverted(status) => {
                BotWorkflowError::SeriesSourceNotConverted(status)
            }
            Self::SeriesNoCurrentSet => BotWorkflowError::SeriesNoCurrentSet,
            Self::SeriesSetNotCurrent { expected, actual } => {
                BotWorkflowError::SeriesSetNotCurrent { expected, actual }
            }
            Self::SeriesGameSideRequired => BotWorkflowError::SeriesGameSideRequired,
            Self::SeriesGameSideMismatch { expected, actual } => {
                BotWorkflowError::SeriesGameSideMismatch { expected, actual }
            }
            Self::InvalidGameResult => BotWorkflowError::InvalidGameResult,
            Self::SeriesGameChampionDuplicate { champion_id } => {
                BotWorkflowError::SeriesGameChampionDuplicate { champion_id }
            }
            Self::SeriesHardFearlessChampionUsed { champion_id } => {
                BotWorkflowError::SeriesHardFearlessChampionUsed { champion_id }
            }
            Self::SeriesPickBanPlayerNotInRoster => {
                BotWorkflowError::SeriesPickBanPlayerNotInRoster
            }
            Self::SeriesNoConfirmedGames => BotWorkflowError::SeriesNoConfirmedGames,
            Self::SeriesEarlyCompletionUnavailable {
                best_of,
                confirmed_game_count,
            } => BotWorkflowError::SeriesEarlyCompletionUnavailable {
                best_of,
                confirmed_game_count,
            },
            Self::SeriesEarlyDrawRequiresTiedScore {
                team_1_score,
                team_2_score,
            } => BotWorkflowError::SeriesEarlyDrawRequiresTiedScore {
                team_1_score,
                team_2_score,
            },
            Self::SeriesGameMmrHistoryAdvanced => BotWorkflowError::SeriesGameMmrHistoryAdvanced,
            Self::SeriesResultUnavailable(status) => {
                BotWorkflowError::SeriesResultUnavailable(status)
            }
            Self::EntryDraftIncomplete {
                joined,
                assigned,
                target,
            } => BotWorkflowError::EntryDraftIncomplete {
                joined,
                assigned,
                target,
            },
            Self::InvalidEntrySlot => BotWorkflowError::InvalidEntrySlot,
            Self::CloseRequiresFullRoster { joined, target } => {
                BotWorkflowError::CloseRequiresFullRoster { joined, target }
            }
            Self::IdempotencyConflict => BotWorkflowError::IdempotencyConflict,
            Self::IdempotencyInProgress => BotWorkflowError::IdempotencyInProgress,
            Self::IdentityConflict => BotWorkflowError::UserIdentityConflict,
            Self::CorruptIdempotentResponse
            | Self::IdempotencyCompletionLost
            | Self::InvariantViolation => BotWorkflowError::InvariantViolation,
        }
    }
}

#[derive(Debug, Error)]
pub enum BotWorkflowError {
    #[error("bot workflow command is invalid: {0}")]
    InvalidCommand(&'static str),
    #[error("recruitment best-of must be 1, 3, or 5, received {received}")]
    InvalidRecruitmentBestOf { received: i16 },
    #[error("auction recruitment {stage} best-of must be 1, 3, or 5, received {received}")]
    InvalidAuctionRecruitmentBestOf { stage: &'static str, received: i16 },
    #[error("10-person auction recruitment cannot declare a semifinal best-of")]
    AuctionRecruitmentSemifinalBestOfUnsupported,
    #[error("20-person auction recruitment requires a semifinal best-of")]
    AuctionRecruitmentSemifinalBestOfRequired,
    #[error("guild was not found")]
    GuildNotFound,
    #[error("guild is not active: {0}")]
    GuildInactive(GuildStatus),
    #[error("guild setup is incomplete")]
    GuildNotConfigured,
    #[error("guild setup was already completed")]
    GuildAlreadyConfigured,
    #[error("guild cannot be configured while its status is {0}")]
    GuildSetupUnavailable(GuildStatus),
    #[error("the configured operator role is required")]
    OperatorRequired,
    #[error("the guild member is inactive")]
    MemberInactive,
    #[error("the caller is not an active member of this guild")]
    MembershipRequired,
    #[error("the recruitment member was not found in this guild")]
    RecruitmentMemberNotFound,
    #[error("the recruitment member is inactive")]
    RecruitmentMemberInactive,
    #[error("the Riot account is already linked to another user")]
    RiotAccountAlreadyLinked,
    #[error("the canonical Riot ID conflicts with another account")]
    RiotIdentityConflict,
    #[error("the Discord identity conflicts with a non-Discord user")]
    UserIdentityConflict,
    #[error("the guild has no active season")]
    NoActiveSeason,
    #[error("the guild has no configured recruitment channel")]
    RecruitmentChannelNotConfigured,
    #[error("recruitment was not found")]
    RecruitmentNotFound,
    #[error("auction tournament was not found")]
    AuctionTournamentNotFound,
    #[error("recruitment must be OPEN, but is {0}")]
    RecruitmentNotOpen(RecruitmentStatus),
    #[error(
        "participant confirmation DMs can only be requested while recruitment is CLOSED, but is {0}"
    )]
    ParticipantConfirmationDmUnavailable(RecruitmentStatus),
    #[error("recruitment message cannot be bound while status is {0}")]
    RecruitmentNotBindable(RecruitmentStatus),
    #[error("recruitment has reached its capacity")]
    RecruitmentFull,
    #[error("auction recruitment requires a declared lane")]
    AuctionDeclaredLaneRequired,
    #[error("participant is already joined")]
    ParticipantAlreadyJoined,
    #[error("participant is not joined")]
    ParticipantNotJoined,
    #[error("preferred lanes must contain one to five unique canonical lanes")]
    InvalidPreferredLanes,
    #[error("entry draft can only be changed while recruitment is CLOSED, but is {0}")]
    EntryDraftNotEditable(RecruitmentStatus),
    #[error("entry draft revision mismatch: expected {expected}, actual {actual}")]
    EntryDraftRevisionConflict { expected: i64, actual: i64 },
    #[error("entry draft source recruitment must be CLOSED for auction conversion, but is {0}")]
    EntryDraftAuctionSourceNotReady(RecruitmentStatus),
    #[error("entry draft source recruitment already has an active series")]
    EntryDraftAlreadyConvertedToSeries,
    #[error("auction setup can only be changed while recruitment is CLOSED, but is {0}")]
    AuctionSetupDraftNotEditable(RecruitmentStatus),
    #[error("auction setup draft revision mismatch: expected {expected}, actual {actual}")]
    AuctionSetupDraftRevisionConflict { expected: i64, actual: i64 },
    #[error("auction setup requires {expected} ordered teams, received {actual}")]
    AuctionSetupTeamCountInvalid { expected: i16, actual: usize },
    #[error("auction setup team index mismatch: expected {expected}, actual {actual}")]
    AuctionSetupTeamIndexInvalid { expected: i16, actual: i16 },
    #[error("auction setup team name is invalid")]
    AuctionSetupTeamNameInvalid,
    #[error("auction setup initial points must be positive")]
    AuctionSetupInitialPointsInvalid,
    #[error("auction setup captains must be unique")]
    AuctionSetupDuplicateCaptain,
    #[error("auction setup captain is not a joined recruitment participant")]
    AuctionSetupCaptainNotJoined,
    #[error("auction setup is incomplete: configured {configured}, required {required}")]
    AuctionSetupDraftIncomplete { configured: i16, required: i16 },
    #[error("auction tournament source recruitment must be CLOSED, but is {0}")]
    AuctionTournamentSourceNotReady(RecruitmentStatus),
    #[error("an active auction tournament already exists for this recruitment")]
    AuctionTournamentAlreadyExists,
    #[error("auction tournament revision mismatch: expected {expected}, actual {actual}")]
    AuctionTournamentRevisionConflict { expected: i64, actual: i64 },
    #[error("auction tournament draft cannot be changed while status is {0}")]
    AuctionTournamentDraftNotEditable(AuctionTournamentStatus),
    #[error("auction tournament draft is locked by bracket or match artifacts")]
    AuctionTournamentDraftLocked,
    #[error("auction draft candidate was not found")]
    AuctionDraftCandidateNotFound,
    #[error("auction draft team was not found")]
    AuctionDraftTeamNotFound,
    #[error("auction draft unsold lot was not found")]
    AuctionDraftUnsoldLotNotFound,
    #[error("auction draft requires an active target")]
    AuctionDraftCurrentTargetRequired,
    #[error("auction draft already has an active target")]
    AuctionDraftCurrentTargetExists,
    #[error("auction draft target does not match the active lot")]
    AuctionDraftCurrentTargetMismatch,
    #[error("auction draft candidate is not in the required state")]
    AuctionDraftCandidateStateMismatch,
    #[error("auction draft has no queued candidate")]
    AuctionDraftNoQueuedCandidate,
    #[error("auction draft team has no free roster slot")]
    AuctionDraftTeamFull,
    #[error(
        "auction bid points are invalid: current={current_points}, submitted={submitted_points}"
    )]
    AuctionDraftBidPointsInvalid {
        current_points: i32,
        submitted_points: i32,
    },
    #[error("auction draft has no action that can be reverted")]
    AuctionDraftNoRevertableAction,
    #[error("auction draft action can no longer be reverted safely")]
    AuctionDraftRevertUnavailable,
    #[error("auction tournament bracket cannot be changed while status is {0}")]
    AuctionTournamentBracketNotEditable(AuctionTournamentStatus),
    #[error("auction tournament bracket is unavailable while status is {0}")]
    AuctionTournamentBracketUnavailable(AuctionTournamentStatus),
    #[error("auction tournament bracket is already locked or has match artifacts")]
    AuctionTournamentBracketLocked,
    #[error("auction tournament bracket recovery is unavailable because match artifacts exist")]
    AuctionTournamentBracketRecoveryUnavailable,
    #[error("auction tournament bracket slot is unavailable for this format")]
    AuctionTournamentBracketSlotUnavailable,
    #[error("auction tournament bracket team roster is incomplete")]
    AuctionTournamentBracketRosterIncomplete,
    #[error("auction tournament bracket slots are incomplete")]
    AuctionTournamentBracketSlotsIncomplete,
    #[error("auction match was not found")]
    AuctionMatchNotFound,
    #[error("auction match is waiting for both bracket teams")]
    AuctionMatchTeamsPending,
    #[error("auction match tournament is not in an active bracket state")]
    AuctionMatchTournamentNotReady,
    #[error("auction match revision mismatch: expected {expected}, actual {actual}")]
    AuctionMatchRevisionConflict { expected: i64, actual: i64 },
    #[error("auction match lineup cannot be changed while status is {0}")]
    AuctionMatchLineupNotEditable(String),
    #[error("auction match lineup is already locked")]
    AuctionMatchLineupLocked,
    #[error("auction match lineup cannot be unlocked while status is {0}")]
    AuctionMatchLineupUnlockUnavailable(String),
    #[error("auction match lineup cannot be unlocked after a game input exists")]
    AuctionMatchLineupResultArtifactsPresent,
    #[error("auction match lineup team is not one of this match's teams")]
    AuctionMatchLineupTeamNotFound,
    #[error("auction match lineup player is not in the active team roster")]
    AuctionMatchLineupMemberNotFound,
    #[error("auction match lineup player is already assigned to another lane")]
    AuctionMatchLineupPlayerAssigned,
    #[error("auction match lineup must contain both teams' five lanes")]
    AuctionMatchLineupIncomplete,
    #[error(
        "both 20-person auction semifinal lineups must be locked before a game result is entered"
    )]
    AuctionMatchSemifinalLineupsIncomplete,
    #[error("auction match result cannot be changed while status is {0}")]
    AuctionMatchResultNotEditable(String),
    #[error("auction match game set mismatch: expected {expected}, actual {actual}")]
    AuctionMatchSetNotCurrent { expected: i16, actual: i16 },
    #[error(
        "the same champion appears more than once in the auction match game input: {champion_id}"
    )]
    AuctionMatchGameChampionDuplicate { champion_id: String },
    #[error("a hard fearless champion was already picked in this auction match: {champion_id}")]
    AuctionMatchHardFearlessChampionUsed { champion_id: String },
    #[error("auction match has no confirmed game to revert")]
    AuctionMatchNoConfirmedGames,
    #[error("a later MMR change prevents this auction match game from being reversed safely")]
    AuctionMatchGameMmrHistoryAdvanced,
    #[error("the downstream final already has lineup or game history")]
    AuctionMatchDownstreamWorkExists,
    #[error("auction tournament result is unavailable while status is {0}")]
    AuctionTournamentResultUnavailable(AuctionTournamentStatus),
    #[error("auction tournament result delivery is currently leased by the bot")]
    AuctionTournamentResultDeliveryInProgress,
    #[error("auction tournament result delivery state is uncertain")]
    AuctionTournamentResultDeliveryUncertain,
    #[error("auction tournament cannot be cancelled while status is {0}")]
    AuctionTournamentCancelUnavailable(AuctionTournamentStatus),
    #[error("a later MMR change prevents this auction tournament from being cancelled safely")]
    AuctionTournamentCancelMmrHistoryAdvanced,
    #[error("recruitment revision mismatch: expected {expected}, actual {actual}")]
    RecruitmentRevisionConflict { expected: i64, actual: i64 },
    #[error("series source recruitment must be CLOSED, but is {0}")]
    SeriesSourceNotReady(RecruitmentStatus),
    #[error("series was not found")]
    SeriesNotFound,
    #[error("series must be IN_PROGRESS, but is {0}")]
    SeriesNotInProgress(SeriesStatus),
    #[error("series revision mismatch: expected {expected}, actual {actual}")]
    SeriesRevisionConflict { expected: i64, actual: i64 },
    #[error("series source recruitment must be CONVERTED, but is {0}")]
    SeriesSourceNotConverted(RecruitmentStatus),
    #[error("series has no current draft set")]
    SeriesNoCurrentSet,
    #[error("series game set mismatch: expected {expected}, actual {actual}")]
    SeriesSetNotCurrent { expected: i16, actual: i16 },
    #[error("series game side must be selected before pick-ban input")]
    SeriesGameSideRequired,
    #[error("series game side mismatch: expected {expected}, actual {actual}")]
    SeriesGameSideMismatch { expected: String, actual: String },
    #[error("series game result input is invalid")]
    InvalidGameResult,
    #[error("the same champion appears more than once in the game input: {champion_id}")]
    SeriesGameChampionDuplicate { champion_id: String },
    #[error("a hard fearless champion was already picked in this series: {champion_id}")]
    SeriesHardFearlessChampionUsed { champion_id: String },
    #[error("pick-ban player is not in the frozen series roster")]
    SeriesPickBanPlayerNotInRoster,
    #[error("series has no confirmed game to revert")]
    SeriesNoConfirmedGames,
    #[error(
        "series early completion requires BO3 or BO5 with at least one confirmed game: best_of={best_of}, confirmed_game_count={confirmed_game_count}"
    )]
    SeriesEarlyCompletionUnavailable {
        best_of: i16,
        confirmed_game_count: i64,
    },
    #[error(
        "series early draw requires a tied score: team_1_score={team_1_score}, team_2_score={team_2_score}"
    )]
    SeriesEarlyDrawRequiresTiedScore {
        team_1_score: i64,
        team_2_score: i64,
    },
    #[error("a later MMR change prevents this game from being reversed safely")]
    SeriesGameMmrHistoryAdvanced,
    #[error("series result is unavailable while status is {0}")]
    SeriesResultUnavailable(SeriesStatus),
    #[error("entry draft must assign all joined participants before series creation")]
    EntryDraftIncomplete {
        joined: i64,
        assigned: i64,
        target: i16,
    },
    #[error("entry draft slot is invalid")]
    InvalidEntrySlot,
    #[error("closing recruitment requires {target} joined participants, found {joined}")]
    CloseRequiresFullRoster { joined: i64, target: i16 },
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict,
    #[error("the same idempotent command is still in progress")]
    IdempotencyInProgress,
    #[error("PostgreSQL bot workflow operation failed")]
    Database(#[source] sqlx::Error),
    #[error("PostgreSQL deadlock retry budget was exhausted")]
    DatabaseBusy,
    #[error("bot workflow response serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("bot workflow storage invariant was violated")]
    InvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_id(value: &str) -> DiscordUserId {
        DiscordUserId::parse(value).expect("valid user ID")
    }

    fn role_id(value: &str) -> DiscordRoleId {
        DiscordRoleId::parse(value).expect("valid role ID")
    }

    fn actor(role_ids: Vec<DiscordRoleId>) -> BotActor {
        BotActor {
            id: user_id("100000000000000001"),
            global_display_name: "Operator Global".to_owned(),
            display_name: "Operator".to_owned(),
            avatar_hash: None,
            role_ids,
        }
    }

    #[test]
    fn direct_message_recipient_skips_development_dummy_users() {
        let discord_user = UserId::parse("100000000000000001").expect("valid Discord user ID");
        let (recipient_user_id, routing_id) = direct_message_recipient(&discord_user)
            .expect("Discord user routing should be valid")
            .expect("Discord user should receive a direct message");
        assert_eq!(recipient_user_id.as_str(), "100000000000000001");
        assert_eq!(routing_id.as_str(), "100000000000000001");

        let dummy_user =
            UserId::parse("dmy_01900000-0000-7000-8000-000000000001").expect("valid dummy");
        assert_eq!(
            direct_message_recipient(&dummy_user).expect("dummy handling should not fail"),
            None
        );
    }

    #[test]
    fn command_routes_are_canonical() {
        let operator = actor(vec![]);
        let recruitment_id = RecruitmentId::new();
        let commands = [
            BotRecruitmentCommand::Create {
                actor: operator.clone(),
                rules: RecruitmentSeriesRules::default(),
                expected_start_at: OffsetDateTime::UNIX_EPOCH,
            },
            BotRecruitmentCommand::BindMessage {
                actor: operator.clone(),
                recruitment_id,
                channel_id: DiscordChannelId::parse("400000000000000001").expect("valid channel"),
                message_id: DiscordMessageId::parse("500000000000000001").expect("valid message"),
            },
            BotRecruitmentCommand::Join {
                actor: operator.clone(),
                recruitment_id,
                participant: operator.clone(),
            },
            BotRecruitmentCommand::Leave {
                actor: operator.clone(),
                recruitment_id,
                user_id: UserId::from(&operator.id),
            },
            BotRecruitmentCommand::SetPreferredLanes {
                actor: operator.clone(),
                recruitment_id,
                user_id: UserId::from(&operator.id),
                lanes: vec![Lane::Mid],
            },
            BotRecruitmentCommand::Close {
                actor: operator,
                recruitment_id,
            },
        ];

        assert_eq!(
            commands.map(|command| command.metadata().route_template),
            [
                "/api/guilds/{guildId}/recruitments",
                "/api/guilds/{guildId}/recruitments/{id}/message",
                "/api/guilds/{guildId}/recruitments/{id}/members",
                "/api/guilds/{guildId}/recruitments/{id}/members/{userId}",
                "/api/guilds/{guildId}/recruitments/{id}/members/{userId}/preferred-lanes",
                "/api/guilds/{guildId}/recruitments/{id}/close",
            ]
        );
    }

    #[test]
    fn auction_participation_routes_are_canonical() {
        let operator = actor(vec![]);
        let recruitment_id = AuctionRecruitmentId::new();
        let commands = [
            BotAuctionRecruitmentParticipationCommand::Join {
                actor: operator.clone(),
                participant: operator.clone(),
                recruitment_id,
                declared_lane: Some(Lane::Mid),
            },
            BotAuctionRecruitmentParticipationCommand::Leave {
                actor: operator.clone(),
                recruitment_id,
                user_id: UserId::from(&operator.id),
            },
        ];

        assert_eq!(
            commands.map(|command| command.metadata().route_template),
            [
                "/api/guilds/{guildId}/auction-recruitments/{id}/members",
                "/api/guilds/{guildId}/auction-recruitments/{id}/members/{userId}",
            ]
        );
    }

    #[test]
    fn web_auction_member_routes_are_canonical() {
        let recruitment_id = AuctionRecruitmentId::new();
        let member_id = UserId::from(&user_id("100000000000000001"));
        let commands = [
            WebAuctionRecruitmentMutation::AddMember {
                recruitment_id,
                member_id: member_id.clone(),
                declared_lane: Lane::Mid,
            },
            WebAuctionRecruitmentMutation::RemoveMember {
                recruitment_id,
                member_id,
            },
        ];

        assert_eq!(
            commands.map(|command| command.metadata().route_template),
            [
                "/api/guilds/{guildId}/auction-recruitments/{id}/members",
                "/api/guilds/{guildId}/auction-recruitments/{id}/members/{memberId}",
            ]
        );
    }

    #[test]
    fn preferred_lanes_are_non_empty_bounded_and_unique() {
        assert!(validate_lanes(&[Lane::Top]).is_ok());
        assert!(validate_lanes(&[]).is_err());
        assert!(validate_lanes(&[Lane::Mid, Lane::Mid]).is_err());
    }

    #[test]
    fn recruitment_rules_are_bounded_and_defaulted() {
        assert_eq!(RecruitmentSeriesRules::default().best_of, 3);
        assert!(RecruitmentSeriesRules::default().hard_fearless);
        assert!(
            validate_recruitment_series_rules(&RecruitmentSeriesRules {
                best_of: 1,
                hard_fearless: false,
            })
            .is_ok()
        );
        assert!(
            validate_recruitment_series_rules(&RecruitmentSeriesRules {
                best_of: 3,
                hard_fearless: true,
            })
            .is_ok()
        );
        assert!(
            validate_recruitment_series_rules(&RecruitmentSeriesRules {
                best_of: 5,
                hard_fearless: false,
            })
            .is_ok()
        );
        assert!(
            validate_recruitment_series_rules(&RecruitmentSeriesRules {
                best_of: 2,
                hard_fearless: false,
            })
            .is_err()
        );
    }

    #[test]
    fn auction_recruitment_rules_are_defaulted_and_format_bound() {
        let ten_person = AuctionRecruitmentSeriesRules::from_optional(
            AuctionFormat::TenPerson,
            None,
            None,
            None,
        );
        assert_eq!(
            ten_person,
            AuctionRecruitmentSeriesRules {
                hard_fearless: true,
                semifinal_best_of: None,
                final_best_of: 3,
            }
        );
        assert!(
            validate_auction_recruitment_series_rules(AuctionFormat::TenPerson, &ten_person)
                .is_ok()
        );

        let twenty_person = AuctionRecruitmentSeriesRules::from_optional(
            AuctionFormat::TwentyPerson,
            Some(1),
            Some(5),
            Some(true),
        );
        assert_eq!(
            twenty_person,
            AuctionRecruitmentSeriesRules {
                hard_fearless: true,
                semifinal_best_of: Some(1),
                final_best_of: 5,
            }
        );
        assert!(
            validate_auction_recruitment_series_rules(AuctionFormat::TwentyPerson, &twenty_person)
                .is_ok()
        );

        let invalid_ten_person = AuctionRecruitmentSeriesRules::from_optional(
            AuctionFormat::TenPerson,
            Some(3),
            Some(3),
            None,
        );
        assert!(matches!(
            validate_auction_recruitment_series_rules(
                AuctionFormat::TenPerson,
                &invalid_ten_person
            ),
            Err(BotWorkflowError::AuctionRecruitmentSemifinalBestOfUnsupported)
        ));
        assert!(matches!(
            validate_auction_recruitment_series_rules(
                AuctionFormat::TwentyPerson,
                &AuctionRecruitmentSeriesRules {
                    hard_fearless: false,
                    semifinal_best_of: Some(3),
                    final_best_of: 2,
                }
            ),
            Err(BotWorkflowError::InvalidAuctionRecruitmentBestOf {
                stage: "final",
                received: 2,
            })
        ));

        assert_eq!(
            serde_json::to_value(twenty_person).expect("auction rules serialize"),
            json!({
                "hardFearless": true,
                "semifinalBestOf": 1,
                "finalBestOf": 5,
            })
        );
    }

    #[test]
    fn auction_tournament_draft_commands_are_strict_and_use_camel_case_fields() {
        let command = serde_json::from_value::<AuctionTournamentDraftCommand>(json!({
            "kind": "AWARD_BID",
            "expectedRevision": 7,
            "memberId": "100000000000000001",
            "teamId": "00000000-0000-0000-0000-000000000001",
            "points": 350
        }))
        .expect("valid auction draft command");
        assert!(matches!(
            command,
            AuctionTournamentDraftCommand::AwardBid {
                expected_revision: 7,
                points: 350,
                ..
            }
        ));
        assert!(
            serde_json::from_value::<AuctionTournamentDraftCommand>(json!({
                "kind": "DRAW_NEXT",
                "expectedRevision": 0,
                "unexpected": true
            }))
            .is_err()
        );
        let reshuffle = serde_json::from_value::<AuctionTournamentDraftCommand>(json!({
            "kind": "RESHUFFLE_CANDIDATES",
            "expectedRevision": 8
        }))
        .expect("valid auction reshuffle command");
        assert!(matches!(
            reshuffle,
            AuctionTournamentDraftCommand::ReshuffleCandidates {
                expected_revision: 8
            }
        ));
        let reset = serde_json::from_value::<AuctionTournamentDraftCommand>(json!({
            "kind": "RESET_TO_INITIAL",
            "expectedRevision": 9
        }))
        .expect("valid auction reset command");
        assert!(matches!(
            reset,
            AuctionTournamentDraftCommand::ResetToInitial {
                expected_revision: 9
            }
        ));
    }

    #[test]
    fn auction_candidate_random_order_never_preserves_source_when_possible() {
        let source = vec![
            UserId::parse("100000000000000001").expect("user 1"),
            UserId::parse("100000000000000002").expect("user 2"),
            UserId::parse("100000000000000003").expect("user 3"),
        ];
        let mut order = source.clone();

        rotate_if_order_preserved(&source, &mut order);

        assert_ne!(order, source);
        assert_eq!(
            order,
            vec![
                UserId::parse("100000000000000002").expect("user 2"),
                UserId::parse("100000000000000003").expect("user 3"),
                UserId::parse("100000000000000001").expect("user 1"),
            ]
        );
    }

    #[test]
    fn auction_tournament_bracket_commands_are_strict_and_use_camel_case_fields() {
        let command = serde_json::from_value::<AuctionTournamentBracketCommand>(json!({
            "kind": "ASSIGN_TEAM",
            "expectedRevision": 7,
            "slotKey": "A_0",
            "teamId": "00000000-0000-0000-0000-000000000001"
        }))
        .expect("valid auction bracket command");
        assert!(matches!(
            command,
            AuctionTournamentBracketCommand::AssignTeam {
                expected_revision: 7,
                ref slot_key,
                ..
            } if slot_key == "A_0"
        ));
        let unlock = serde_json::from_value::<AuctionTournamentBracketCommand>(json!({
            "kind": "UNLOCK_BRACKET",
            "expectedRevision": 8
        }))
        .expect("valid auction bracket unlock command");
        assert!(matches!(
            unlock,
            AuctionTournamentBracketCommand::UnlockBracket {
                expected_revision: 8
            }
        ));
        assert!(
            serde_json::from_value::<AuctionTournamentBracketCommand>(json!({
                "kind": "LOCK_BRACKET",
                "expectedRevision": 0,
                "unexpected": true
            }))
            .is_err()
        );
    }

    #[test]
    fn auction_tournament_cancel_requires_a_valid_revision_and_reason() {
        let valid = AuctionTournamentCancelCommand {
            actor: actor(vec![]),
            expected_revision: 3,
            reason: "운영자 요청으로 경매 내전을 종료합니다.".to_owned(),
        };
        assert!(validate_auction_tournament_cancel_command(&valid).is_ok());

        let empty_reason = AuctionTournamentCancelCommand {
            reason: "   ".to_owned(),
            ..valid.clone()
        };
        assert!(validate_auction_tournament_cancel_command(&empty_reason).is_err());

        let stale_revision = AuctionTournamentCancelCommand {
            expected_revision: -1,
            ..valid
        };
        assert!(validate_auction_tournament_cancel_command(&stale_revision).is_err());
    }

    #[test]
    fn auction_match_game_commands_are_strict_and_use_canonical_routes() {
        let command = serde_json::from_value::<AuctionMatchGameSubmitCommand>(json!({
            "expectedRevision": 4,
            "setNumber": 2,
            "sidePreset": "DEFAULT",
            "winner": "BLUE",
            "picks": {
                "100000000000000001": "Ahri"
            },
            "bans": {
                "BLUE": [null, null, null, null, null],
                "RED": [null, null, null, null, null]
            },
            "patchVersion": null
        }))
        .expect("valid auction match game command");
        assert!(validate_auction_match_game_submit_command(&command).is_ok());
        assert!(
            serde_json::from_value::<AuctionMatchGameSubmitCommand>(json!({
                "expectedRevision": 4,
                "setNumber": 1,
                "sidePreset": "DEFAULT",
                "winner": "BLUE",
                "picks": {},
                "bans": { "BLUE": [], "RED": [] },
                "unexpected": true
            }))
            .is_err()
        );

        let mutation = WebAuctionMatchGameMutation::Submit(command);
        assert_eq!(mutation.method(), "POST");
        assert_eq!(mutation.route_template(), AUCTION_MATCH_GAME_SUBMIT_ROUTE);
        let revert = WebAuctionMatchGameMutation::Revert(AuctionMatchGameRevertCommand {
            expected_revision: 4,
        });
        assert_eq!(revert.method(), "DELETE");
        assert_eq!(revert.route_template(), AUCTION_MATCH_GAME_REVERT_ROUTE);
    }

    #[test]
    fn twenty_person_semifinal_result_gate_excludes_finals_and_ten_person_matches() {
        assert!(requires_twenty_person_semifinal_lineup_gate(
            "TWENTY_PERSON",
            "SEMIFINAL_A"
        ));
        assert!(requires_twenty_person_semifinal_lineup_gate(
            "TWENTY_PERSON",
            "SEMIFINAL_B"
        ));
        assert!(!requires_twenty_person_semifinal_lineup_gate(
            "TWENTY_PERSON",
            "FINAL"
        ));
        assert!(!requires_twenty_person_semifinal_lineup_gate(
            "TEN_PERSON",
            "FINAL"
        ));
    }

    #[test]
    fn auction_match_result_permission_requires_all_semifinal_lineups() {
        assert!(!can_enter_auction_match_results(true, true, false, "READY"));
        assert!(can_enter_auction_match_results(true, true, true, "READY"));
        assert!(can_enter_auction_match_results(
            true,
            true,
            true,
            "IN_PROGRESS"
        ));
        assert!(!can_enter_auction_match_results(
            true,
            true,
            true,
            "COMPLETED"
        ));
        assert!(!can_enter_auction_match_results(false, true, true, "READY"));
    }

    #[test]
    fn auction_lineup_confirmation_snapshot_is_lane_ordered_and_immutable() {
        let guild_id = GuildId::parse("200000000000000001").expect("guild ID");
        let tournament_id = AuctionTournamentId::from_uuid(
            Uuid::parse_str("01900000-0000-7000-8000-000000000501").expect("tournament ID"),
        );
        let match_id = AuctionMatchId::from_uuid(
            Uuid::parse_str("01900000-0000-7000-8000-000000000601").expect("match ID"),
        );
        let left_team_id =
            Uuid::parse_str("01900000-0000-7000-8000-000000000701").expect("left team ID");
        let right_team_id =
            Uuid::parse_str("01900000-0000-7000-8000-000000000702").expect("right team ID");
        let lanes = [
            Lane::Top,
            Lane::Jungle,
            Lane::Mid,
            Lane::Bottom,
            Lane::Support,
        ];
        let team = |team_id: Uuid, name: &str, starting_user: u8| AuctionMatchSnapshotTeam {
            id: team_id,
            team_name: name.to_owned(),
            members: lanes
                .iter()
                .copied()
                .enumerate()
                .map(|(index, lane)| {
                    let user_number = starting_user + u8::try_from(index).expect("lane index");
                    AuctionTournamentTeamMemberSnapshot {
                        team_member_id: Uuid::from_u128(u128::from(user_number)),
                        user_id: UserId::parse(format!("1000000000000000{user_number:02}"))
                            .expect("user ID"),
                        display_name: format!("{name} {lane}"),
                        declared_lane: Some(lane),
                        mmr_at_acquisition: Some(1500.0),
                        snapshot_origin: "TEST".to_owned(),
                        acquired_via: "TEST".to_owned(),
                        roster_slot: i16::try_from(index + 1).expect("roster slot"),
                    }
                })
                .collect(),
        };
        let left_team = team(left_team_id, "블루팀", 1);
        let right_team = team(right_team_id, "레드팀", 6);
        let lineups = [&left_team, &right_team]
            .into_iter()
            .flat_map(|team| {
                team.members.iter().zip(lanes).map(move |(member, lane)| {
                    AuctionMatchLineupAssignmentSnapshot {
                        team_id: team.id,
                        lane,
                        team_member_id: member.team_member_id,
                        user_id: member.user_id.clone(),
                    }
                })
            })
            .collect();
        let snapshot = AuctionMatchSnapshot {
            r#match: AuctionMatchSnapshotMatch {
                id: match_id,
                tournament_id,
                match_key: "FINAL".to_owned(),
                label: "결승".to_owned(),
                best_of: 3,
                hard_fearless: true,
                status: "READY".to_owned(),
                revision: 14,
                left_team_id,
                right_team_id,
                left_score: 0,
                right_score: 0,
                winner_team_id: None,
                lineups_locked: true,
            },
            teams: vec![left_team, right_team],
            lineups,
            games: Vec::new(),
            permissions: AuctionMatchSnapshotPermissions {
                can_manage_lineup: false,
                can_enter_results: true,
                can_unlock_lineup: true,
                can_revert_last_game: false,
            },
        };
        let tournament = AuctionTournamentLockRow {
            display_number: 42,
            title: "금요 경매 내전".to_owned(),
            format: "TWENTY_PERSON".to_owned(),
            hard_fearless: true,
            status: AuctionTournamentStatus::InGame,
            revision: 8,
            bracket_locked_at: Some(OffsetDateTime::now_utc()),
        };

        let confirmation =
            auction_match_lineup_confirmation_snapshot(&guild_id, &tournament, &snapshot)
                .expect("complete lock snapshot creates a confirmation card");
        let payload = serde_json::to_value(&confirmation).expect("confirmation serializes");

        assert_eq!(payload["tournament"]["format"], "TWENTY_PERSON");
        assert_eq!(payload["match"]["id"], match_id.to_string());
        assert_eq!(payload["teams"][0]["teamId"], left_team_id.to_string());
        assert_eq!(payload["teams"][1]["teamId"], right_team_id.to_string());
        assert_eq!(payload["teams"][0]["players"][0]["lane"], "TOP");
        assert_eq!(payload["teams"][0]["players"][4]["lane"], "SUPPORT");
        assert_eq!(
            payload["links"]["pickBan"],
            format!("/g/{guild_id}/auction/matches/{match_id}")
        );
    }

    #[test]
    fn auction_match_game_revert_permission_honors_downstream_final_work() {
        assert!(can_revert_auction_match_last_game(
            true,
            true,
            true,
            "COMPLETED",
            false,
        ));
        assert!(!can_revert_auction_match_last_game(
            true,
            true,
            true,
            "COMPLETED",
            true,
        ));
        assert!(!can_revert_auction_match_last_game(
            true,
            true,
            false,
            "COMPLETED",
            false,
        ));
    }

    #[test]
    fn auction_match_set_progression_stops_at_the_saved_best_of() {
        assert_eq!(next_auction_match_set_number(0, 3).expect("set 1"), 1);
        assert_eq!(next_auction_match_set_number(1, 3).expect("set 2"), 2);
        assert_eq!(next_auction_match_set_number(2, 3).expect("set 3"), 3);
        assert!(next_auction_match_set_number(3, 3).is_err());
        assert_eq!(next_auction_match_set_number(0, 1).expect("bo1 set"), 1);
    }

    #[test]
    fn reverted_unsold_assignment_can_be_restored_without_reusing_the_lot() {
        let user_id = UserId::from(&user_id("100000000000000001"));
        let lots = vec![AuctionLotLockRow {
            id: Uuid::nil(),
            draw_sequence: 1,
            target_user_id: user_id.clone(),
            status: "REVERTED".to_owned(),
        }];

        assert!(auction_unsold_lot(&lots, &user_id).is_err());
        assert_eq!(
            auction_unsold_history(&lots, &user_id)
                .expect("reverted lot is sufficient history for restore")
                .id,
            Uuid::nil()
        );
    }

    #[test]
    fn early_completion_requires_a_played_bo3_or_bo5() {
        assert!(!can_early_complete_series(1, 1));
        assert!(!can_early_complete_series(3, 0));
        assert!(can_early_complete_series(3, 1));
        assert!(can_early_complete_series(5, 1));
    }

    #[test]
    fn pick_ban_bulk_is_bounded_and_only_accepts_semantic_set_items() {
        let member = UserId::from(&user_id("100000000000000001"));
        let set_pick = SeriesPickBanBulkItem::SetPick {
            player_id: member,
            champion_id: "Ahri".to_owned(),
        };
        assert!(validate_series_pick_ban_bulk_items(&[set_pick.clone()]));
        assert!(!validate_series_pick_ban_bulk_items(&[]));
        assert!(!validate_series_pick_ban_bulk_items(&vec![
            set_pick.clone();
            21
        ]));
        assert!(!validate_series_pick_ban_bulk_items(&[
            set_pick.clone(),
            set_pick.clone(),
        ]));
        assert!(!validate_series_pick_ban_bulk_items(&[
            SeriesPickBanBulkItem::SetPick {
                player_id: UserId::from(&user_id("100000000000000001")),
                champion_id: "Jade_Ahri".to_owned(),
            },
        ]));
        assert!(
            serde_json::from_value::<SeriesPickBanOperation>(json!({
                "type": "APPLY_BULK",
                "items": [{
                    "type": "CLEAR_PICK",
                    "playerId": "100000000000000001"
                }]
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SeriesPickBanOperation>(json!({
                "type": "SET_HARD_FEARLESS",
                "hardFearless": true
            }))
            .is_err()
        );
    }

    #[test]
    fn compact_riot_tier_only_exposes_canonical_solo_rank_values() {
        assert_eq!(
            compact_riot_tier(Some("PLATINUM"), Some("III")),
            Some("P3".to_owned())
        );
        assert_eq!(
            compact_riot_tier(Some("grandmaster"), None),
            Some("GM".to_owned())
        );
        assert_eq!(
            compact_riot_tier(Some("CHALLENGER"), Some("I")),
            Some("C1".to_owned())
        );
        assert_eq!(compact_riot_tier(Some("UNRANKED"), None), None);
        assert_eq!(compact_riot_tier(Some("PLATINUM"), Some("V")), None);
        assert_eq!(compact_riot_tier(None, Some("III")), None);
    }

    #[test]
    fn self_service_or_operator_role_is_required() {
        let operator_role = role_id("300000000000000001");
        let target = user_id("100000000000000002");
        let self_actor = actor(vec![]);
        assert!(
            require_self_or_operator(&self_actor, &UserId::from(&self_actor.id), &operator_role)
                .is_ok()
        );
        assert!(
            require_self_or_operator(&self_actor, &UserId::from(&target), &operator_role).is_err()
        );

        let operator = actor(vec![operator_role.clone()]);
        assert!(
            require_self_or_operator(&operator, &UserId::from(&target), &operator_role).is_ok()
        );
    }
}
