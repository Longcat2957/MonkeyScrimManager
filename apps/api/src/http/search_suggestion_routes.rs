use std::str::FromStr as _;

use axum::{
    Extension,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    domain::GuildId,
    error::{self, ApiError},
    http::extractors::AuthenticatedSession,
    integrations::riot::RiotPlatform,
    repository::{
        riot::normalize_riot_id_v1,
        search_suggestions::{
            self, GuildMemberSuggestionRecord, GuildMemberSuggestionSearch,
            RiotAccountSuggestionRecord, SearchSuggestionRepositoryError,
        },
    },
    state::{AppState, RequestId},
};

const DEFAULT_LIMIT: usize = 8;
const MAX_LIMIT: usize = 8;
const MIN_QUERY_CHARS: usize = 2;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicRiotSuggestionQuery {
    q: String,
    #[serde(default = "default_platform")]
    platform: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GuildSuggestionQuery {
    q: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Serialize)]
struct SuggestionsData {
    suggestions: Vec<SuggestionData>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SuggestionData {
    kind: &'static str,
    value: String,
    label: String,
    description: Option<String>,
    profile_icon_id: Option<i32>,
}

pub async fn public_riot_suggestions(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<PublicRiotSuggestionQuery>,
) -> Result<Response, ApiError> {
    let limit = validate_limit(query.limit, &request_id)?;
    let normalized_prefix = normalize_search_query(&query.q, &request_id)?;
    let platform = RiotPlatform::from_str(&query.platform).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            &request_id,
            "invalid_region",
            "지원하지 않는 Riot 플랫폼입니다.",
            Some(serde_json::json!({ "platform": query.platform })),
        )
    })?;
    let suggestions = search_suggestions::public_riot_accounts(
        database(&state, &request_id)?,
        platform.as_str(),
        &normalized_prefix,
        limit,
    )
    .await
    .map_err(|error| repository_error(error, &request_id))?
    .into_iter()
    .map(map_riot_account)
    .collect();

    Ok(success(suggestions, &request_id))
}

pub async fn guild_member_suggestions(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    principal: AuthenticatedSession,
    Query(query): Query<GuildSuggestionQuery>,
) -> Result<Response, ApiError> {
    let limit = validate_limit(query.limit, &request_id)?;
    let prefix = trimmed_search_query(&query.q, &request_id)?;
    let result = search_suggestions::guild_members(
        database(&state, &request_id)?,
        &principal.session.user_id,
        &guild_id,
        prefix,
        limit,
    )
    .await
    .map_err(|error| repository_error(error, &request_id))?;
    let GuildMemberSuggestionSearch::Allowed(records) = result else {
        return Err(hidden_guild_not_found(&request_id));
    };
    let suggestions = records.into_iter().map(map_guild_member).collect();

    Ok(success(suggestions, &request_id))
}

fn map_riot_account(record: RiotAccountSuggestionRecord) -> SuggestionData {
    let riot_id = format!("{}#{}", record.game_name, record.tag_line);
    SuggestionData {
        kind: "riotAccount",
        value: riot_id.clone(),
        label: riot_id,
        description: Some(record.platform),
        profile_icon_id: record.profile_icon_id,
    }
}

fn map_guild_member(record: GuildMemberSuggestionRecord) -> SuggestionData {
    let description = record
        .game_name
        .zip(record.tag_line)
        .map(|(game_name, tag_line)| format!("{game_name}#{tag_line}"));
    SuggestionData {
        kind: "guildMember",
        value: record.display_name.clone(),
        label: record.display_name,
        description,
        profile_icon_id: record.profile_icon_id,
    }
}

fn success(suggestions: Vec<SuggestionData>, request_id: &RequestId) -> Response {
    error::success(StatusCode::OK, request_id, SuggestionsData { suggestions })
}

fn normalize_search_query(value: &str, request_id: &RequestId) -> Result<String, ApiError> {
    let normalized = normalize_riot_id_v1(value);
    validate_query_length(&normalized, request_id)?;
    Ok(normalized)
}

fn trimmed_search_query<'a>(value: &'a str, request_id: &RequestId) -> Result<&'a str, ApiError> {
    let trimmed = value.trim();
    validate_query_length(trimmed, request_id)?;
    Ok(trimmed)
}

fn validate_query_length(value: &str, request_id: &RequestId) -> Result<(), ApiError> {
    if value.chars().count() < MIN_QUERY_CHARS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_search_query",
            "검색어를 두 글자 이상 입력해 주세요.",
            Some(serde_json::json!({ "minLength": MIN_QUERY_CHARS })),
        ));
    }
    Ok(())
}

fn validate_limit(limit: usize, request_id: &RequestId) -> Result<i64, ApiError> {
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_search_limit",
            "검색 결과 개수 설정이 올바르지 않습니다.",
            Some(serde_json::json!({ "maxLimit": MAX_LIMIT })),
        ));
    }
    i64::try_from(limit).map_err(|_| internal(request_id))
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

fn repository_error(error: SearchSuggestionRepositoryError, request_id: &RequestId) -> ApiError {
    tracing::error!(%error, "search suggestion repository operation failed");
    match error {
        SearchSuggestionRepositoryError::Sql(source) => {
            error::database_failure(&source, request_id)
        }
    }
}

fn hidden_guild_not_found(request_id: &RequestId) -> ApiError {
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

const fn default_limit() -> usize {
    DEFAULT_LIMIT
}

fn default_platform() -> String {
    "KR".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_id() -> RequestId {
        RequestId("request-id".to_owned())
    }

    #[test]
    fn riot_search_query_uses_the_existing_identity_normalization() {
        assert_eq!(
            normalize_search_query("  ＬｏｎｇＣａｔ#ＫＲ１ ", &request_id()).expect("valid query"),
            "longcat#kr1"
        );
    }

    #[test]
    fn query_and_limit_boundaries_are_enforced() {
        assert!(normalize_search_query("가", &request_id()).is_err());
        assert!(trimmed_search_query(" a ", &request_id()).is_err());
        assert_eq!(validate_limit(1, &request_id()).expect("minimum"), 1);
        assert_eq!(validate_limit(8, &request_id()).expect("maximum"), 8);
        assert!(validate_limit(0, &request_id()).is_err());
        assert!(validate_limit(9, &request_id()).is_err());
    }

    #[test]
    fn response_mapping_never_contains_a_private_identifier() {
        let riot = map_riot_account(RiotAccountSuggestionRecord {
            platform: "KR".to_owned(),
            game_name: "Longcat".to_owned(),
            tag_line: "KR1".to_owned(),
            profile_icon_id: Some(7),
        });
        assert_eq!(riot.kind, "riotAccount");
        assert_eq!(riot.value, "Longcat#KR1");
        assert_eq!(riot.label, "Longcat#KR1");
        assert_eq!(riot.description.as_deref(), Some("KR"));

        let hidden_member = map_guild_member(GuildMemberSuggestionRecord {
            display_name: "Alpha".to_owned(),
            game_name: None,
            tag_line: None,
            profile_icon_id: None,
        });
        assert_eq!(hidden_member.kind, "guildMember");
        assert_eq!(hidden_member.description, None);
        assert_eq!(hidden_member.profile_icon_id, None);
    }
}
