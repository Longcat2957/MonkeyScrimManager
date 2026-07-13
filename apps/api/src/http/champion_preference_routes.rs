use std::str::FromStr as _;

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    domain::Lane,
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, require_csrf},
    repository::champion_preferences::{
        self, ChampionPreferencesRepositoryError, ChampionPreferencesSnapshot,
        MAX_CHAMPIONS_PER_LANE,
    },
    state::{AppState, RequestId},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChampionPreferencesUpdateRequest {
    champion_ids: Vec<String>,
}

#[derive(Serialize)]
struct ChampionPreferencesData {
    preferences: ChampionPreferencesResponse,
}

#[derive(Serialize)]
struct ChampionPreferencesResponse {
    #[serde(rename = "TOP")]
    top: Vec<String>,
    #[serde(rename = "JUNGLE")]
    jungle: Vec<String>,
    #[serde(rename = "MID")]
    mid: Vec<String>,
    #[serde(rename = "BOTTOM")]
    bottom: Vec<String>,
    #[serde(rename = "SUPPORT")]
    support: Vec<String>,
}

pub async fn get_champion_preferences(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let snapshot =
        champion_preferences::get(database(&state, &request_id)?, &principal.session.user_id)
            .await
            .map_err(|source| repository_error(&source, &request_id))?;
    Ok(snapshot_response(&request_id, snapshot))
}

pub async fn update_champion_preferences(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Path(lane): Path<String>,
    Json(request): Json<ChampionPreferencesUpdateRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let lane = parse_lane(&lane, &request_id)?;
    let snapshot = champion_preferences::replace_lane(
        database(&state, &request_id)?,
        &principal.session.user_id,
        lane,
        request.champion_ids,
    )
    .await
    .map_err(|source| repository_error(&source, &request_id))?;
    Ok(snapshot_response(&request_id, snapshot))
}

fn snapshot_response(request_id: &RequestId, snapshot: ChampionPreferencesSnapshot) -> Response {
    error::success(
        StatusCode::OK,
        request_id,
        ChampionPreferencesData {
            preferences: ChampionPreferencesResponse {
                top: snapshot.top,
                jungle: snapshot.jungle,
                mid: snapshot.mid,
                bottom: snapshot.bottom,
                support: snapshot.support,
            },
        },
    )
}

fn parse_lane(value: &str, request_id: &RequestId) -> Result<Lane, ApiError> {
    Lane::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_lane",
            "지원하지 않는 포지션입니다.",
            Some(serde_json::json!({ "lane": value })),
        )
    })
}

fn database<'a>(state: &'a AppState, request_id: &RequestId) -> Result<&'a PgPool, ApiError> {
    state.postgres().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_unavailable",
            "데이터베이스를 사용할 수 없습니다.",
            None,
        )
    })
}

fn repository_error(
    source: &ChampionPreferencesRepositoryError,
    request_id: &RequestId,
) -> ApiError {
    match source {
        ChampionPreferencesRepositoryError::Sql(source) => {
            error::database_failure(source, request_id)
        }
        ChampionPreferencesRepositoryError::TooManyChampions => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "too_many_champion_preferences",
            "한 포지션에는 선호 챔피언을 최대 10개까지 저장할 수 있습니다.",
            Some(serde_json::json!({ "max": MAX_CHAMPIONS_PER_LANE })),
        ),
        ChampionPreferencesRepositoryError::InvalidChampionId => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_champion_id",
            "챔피언 ID 형식이 올바르지 않습니다.",
            None,
        ),
        ChampionPreferencesRepositoryError::DuplicateChampionId => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "duplicate_champion_id",
            "같은 챔피언을 한 포지션에 중복해서 저장할 수 없습니다.",
            None,
        ),
        ChampionPreferencesRepositoryError::UserNotFound
        | ChampionPreferencesRepositoryError::StoredInvariantViolation => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            request_id,
            "internal_error",
            "요청을 처리하는 중 내부 오류가 발생했습니다.",
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_parser_accepts_only_canonical_wire_values() {
        let request_id = RequestId("test-request".to_owned());
        for value in ["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"] {
            assert!(parse_lane(value, &request_id).is_ok());
        }
        for value in ["top", "FLEX", ""] {
            assert!(parse_lane(value, &request_id).is_err());
        }
    }

    #[test]
    fn update_request_is_strict_and_uses_camel_case() {
        assert!(
            serde_json::from_str::<ChampionPreferencesUpdateRequest>(r#"{"championIds":["Ahri"]}"#)
                .is_ok()
        );
        assert!(
            serde_json::from_str::<ChampionPreferencesUpdateRequest>(
                r#"{"championIds":[],"lane":"MID"}"#
            )
            .is_err()
        );
    }
}
