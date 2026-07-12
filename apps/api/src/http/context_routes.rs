use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::format_description::well_known::Rfc3339;

use crate::{
    domain::{DiscordUserId, GuildId},
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, require_csrf},
    repository::context::{self, CurrentSeasonRecord, GuildContextRecord, GuildSummaryRecord},
    state::{AppState, RequestId},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MeData {
    discord_id: DiscordUserId,
    display_name: String,
    avatar_url: Option<String>,
    default_guild_id: Option<GuildId>,
    service_admin: bool,
}

#[derive(Serialize)]
struct GuildListData {
    guilds: Vec<GuildSummaryData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildSummaryData {
    id: GuildId,
    display_name: String,
    icon_url: Option<String>,
    member_count: i64,
    current_user_role: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildContextData {
    id: GuildId,
    display_name: String,
    icon_url: Option<String>,
    member_count: i64,
    current_user_role: &'static str,
    is_externally_public: bool,
    current_season: Option<CurrentSeasonData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CurrentSeasonData {
    id: String,
    name: String,
    started_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetDefaultGuildRequest {
    guild_id: GuildId,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetDefaultGuildData {
    default_guild_id: GuildId,
}

pub async fn me(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let session = principal.session;
    let service_admin = state
        .config()
        .auth()
        .is_some_and(|auth| auth.is_service_admin(&session.user_id));

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        MeData {
            avatar_url: session
                .avatar_hash
                .as_deref()
                .and_then(|hash| discord_cdn_url("avatars", session.user_id.as_str(), hash)),
            discord_id: session.user_id,
            display_name: session.display_name,
            default_guild_id: session.default_guild_id,
            service_admin,
        },
    ))
}

pub async fn me_guilds(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let guilds = context::list_guilds(database(&state, &request_id)?, &principal.session.user_id)
        .await
        .map_err(|error| repository_error(error, &request_id))?
        .into_iter()
        .map(map_guild_summary)
        .collect();

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        GuildListData { guilds },
    ))
}

pub async fn set_default_guild(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<SetDefaultGuildRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let record = context::set_default_guild(
        database(&state, &request_id)?,
        &principal.session.user_id,
        &request.guild_id,
    )
    .await
    .map_err(|error| repository_error(error, &request_id))?
    .ok_or_else(|| hidden_not_found(&request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        SetDefaultGuildData {
            default_guild_id: record.default_guild_id,
        },
    ))
}

pub async fn guild_context(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let record = context::get_guild(
        database(&state, &request_id)?,
        &principal.session.user_id,
        &guild_id,
    )
    .await
    .map_err(|error| repository_error(error, &request_id))?
    .ok_or_else(|| hidden_not_found(&request_id))?;
    let data = map_guild_context(record, &request_id)?;

    Ok(error::success(StatusCode::OK, &request_id, data))
}

fn map_guild_summary(record: GuildSummaryRecord) -> GuildSummaryData {
    GuildSummaryData {
        icon_url: record
            .icon_hash
            .as_deref()
            .and_then(|hash| discord_cdn_url("icons", record.id.as_str(), hash)),
        id: record.id,
        display_name: record.display_name,
        member_count: record.member_count,
        current_user_role: record.current_user_role.as_str(),
    }
}

fn map_guild_context(
    record: GuildContextRecord,
    request_id: &RequestId,
) -> Result<GuildContextData, ApiError> {
    let current_season = record
        .current_season
        .map(|season| map_current_season(season, request_id))
        .transpose()?;

    Ok(GuildContextData {
        icon_url: record
            .icon_hash
            .as_deref()
            .and_then(|hash| discord_cdn_url("icons", record.id.as_str(), hash)),
        id: record.id,
        display_name: record.display_name,
        member_count: record.member_count,
        current_user_role: record.current_user_role.as_str(),
        is_externally_public: record.is_externally_public,
        current_season,
    })
}

fn map_current_season(
    record: CurrentSeasonRecord,
    request_id: &RequestId,
) -> Result<CurrentSeasonData, ApiError> {
    let started_at = record.started_at.format(&Rfc3339).map_err(|error| {
        tracing::error!(%error, "failed to format active season timestamp");
        internal(request_id)
    })?;
    Ok(CurrentSeasonData {
        id: record.id.to_string(),
        name: record.name,
        started_at,
    })
}

fn discord_cdn_url(kind: &str, id: &str, hash: &str) -> Option<String> {
    let valid_hash = !hash.is_empty()
        && hash.len() <= 128
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'_');
    valid_hash.then(|| format!("https://cdn.discordapp.com/{kind}/{id}/{hash}.webp?size=128"))
}

fn database<'a>(state: &'a AppState, request_id: &RequestId) -> Result<&'a PgPool, ApiError> {
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

fn repository_error(error: context::ContextRepositoryError, request_id: &RequestId) -> ApiError {
    tracing::error!(%error, "guild context repository operation failed");
    match error {
        context::ContextRepositoryError::Sql(source) => {
            error::database_failure(&source, request_id)
        }
        context::ContextRepositoryError::IncompleteCurrentSeason => internal(request_id),
    }
}

fn hidden_not_found(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        request_id,
        "not_found",
        "요청한 길드를 찾을 수 없습니다.",
        None,
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_cdn_urls_accept_only_provider_hashes() {
        assert_eq!(
            discord_cdn_url("icons", "123456789012345678", "a_0123abcdef").as_deref(),
            Some("https://cdn.discordapp.com/icons/123456789012345678/a_0123abcdef.webp?size=128")
        );
        assert_eq!(
            discord_cdn_url("icons", "123456789012345678", "../../secret"),
            None
        );
    }
}
