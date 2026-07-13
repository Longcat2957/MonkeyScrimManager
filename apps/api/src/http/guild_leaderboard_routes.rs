use axum::{
    Extension,
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};
use serde::Serialize;

use crate::{
    domain::{GuildId, Lane},
    error::{self, ApiError},
    http::extractors::AuthenticatedSession,
    repository::guild_leaderboard::{
        self, GuildLeaderboardPlayer, GuildLeaderboardRepositoryError, GuildLeaderboardRoleStat,
        GuildLeaderboardSeason, GuildLeaderboardSnapshot,
    },
    state::{AppState, RequestId},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildLeaderboardData {
    season: Option<GuildLeaderboardSeasonData>,
    players: Vec<GuildLeaderboardPlayerData>,
}

#[derive(Serialize)]
struct GuildLeaderboardSeasonData {
    id: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildLeaderboardPlayerData {
    id: String,
    name: String,
    tag: Option<String>,
    profile_icon_id: Option<i32>,
    composite_mmr: f64,
    main_role: Lane,
    is_me: bool,
    stats: GuildLeaderboardStatsData,
}

#[derive(Default, Serialize)]
struct GuildLeaderboardStatsData {
    #[serde(rename = "TOP", skip_serializing_if = "Option::is_none")]
    top: Option<GuildLeaderboardRoleStatData>,
    #[serde(rename = "JUNGLE", skip_serializing_if = "Option::is_none")]
    jungle: Option<GuildLeaderboardRoleStatData>,
    #[serde(rename = "MID", skip_serializing_if = "Option::is_none")]
    mid: Option<GuildLeaderboardRoleStatData>,
    #[serde(rename = "BOTTOM", skip_serializing_if = "Option::is_none")]
    bottom: Option<GuildLeaderboardRoleStatData>,
    #[serde(rename = "SUPPORT", skip_serializing_if = "Option::is_none")]
    support: Option<GuildLeaderboardRoleStatData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildLeaderboardRoleStatData {
    mmr: f64,
    games: i32,
    wins: i32,
    champion_id: Option<String>,
}

pub async fn leaderboard(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let snapshot = guild_leaderboard::get(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    let data = GuildLeaderboardData::from_snapshot(snapshot).map_err(|()| {
        tracing::error!("guild leaderboard response projection violated an invariant");
        internal(&request_id)
    })?;
    Ok(error::success(StatusCode::OK, &request_id, data))
}

impl GuildLeaderboardData {
    fn from_snapshot(snapshot: GuildLeaderboardSnapshot) -> Result<Self, ()> {
        Ok(Self {
            season: snapshot.season.map(GuildLeaderboardSeasonData::from),
            players: snapshot
                .players
                .into_iter()
                .map(GuildLeaderboardPlayerData::try_from)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl From<GuildLeaderboardSeason> for GuildLeaderboardSeasonData {
    fn from(season: GuildLeaderboardSeason) -> Self {
        Self {
            id: season.id.to_string(),
            name: season.name,
        }
    }
}

impl TryFrom<GuildLeaderboardPlayer> for GuildLeaderboardPlayerData {
    type Error = ();

    fn try_from(player: GuildLeaderboardPlayer) -> Result<Self, Self::Error> {
        let mut stats = GuildLeaderboardStatsData::default();
        for stat in player.stats {
            stats.insert(stat)?;
        }
        Ok(Self {
            id: player.id.to_string(),
            name: player.name,
            tag: player.tag,
            profile_icon_id: player.profile_icon_id,
            composite_mmr: player.composite_mmr,
            main_role: player.main_role,
            is_me: player.is_me,
            stats,
        })
    }
}

impl GuildLeaderboardStatsData {
    fn insert(&mut self, stat: GuildLeaderboardRoleStat) -> Result<(), ()> {
        let lane = stat.lane;
        let target = match lane {
            Lane::Top => &mut self.top,
            Lane::Jungle => &mut self.jungle,
            Lane::Mid => &mut self.mid,
            Lane::Bottom => &mut self.bottom,
            Lane::Support => &mut self.support,
        };
        if target.is_some() {
            return Err(());
        }
        *target = Some(GuildLeaderboardRoleStatData {
            mmr: stat.mmr,
            games: stat.games,
            wins: stat.wins,
            champion_id: stat.champion_id,
        });
        Ok(())
    }
}

fn database<'a>(state: &'a AppState, request_id: &RequestId) -> Result<&'a sqlx::PgPool, ApiError> {
    state.postgres().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_unavailable",
            "데이터베이스를 일시적으로 사용할 수 없습니다.",
            None,
        )
    })
}

fn repository_error(source: GuildLeaderboardRepositoryError, request_id: &RequestId) -> ApiError {
    match source {
        GuildLeaderboardRepositoryError::GuildNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 길드를 찾을 수 없습니다.",
            None,
        ),
        GuildLeaderboardRepositoryError::StoredInvariantViolation => {
            tracing::error!("guild leaderboard stored invariant failed");
            internal(request_id)
        }
        GuildLeaderboardRepositoryError::Database(database_error) => {
            tracing::error!(%database_error, "guild leaderboard database operation failed");
            error::database_failure(&database_error, request_id)
        }
    }
}

fn internal(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        request_id,
        "internal_error",
        "요청을 처리하는 중 내부 오류가 발생했습니다.",
        None,
    )
}
