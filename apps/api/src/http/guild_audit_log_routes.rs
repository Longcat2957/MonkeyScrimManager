use axum::{
    Extension,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::GuildId,
    error::{self, ApiError},
    http::extractors::AuthenticatedSession,
    repository::guild_audit_logs::{
        self, AuditLogCursor, GuildAuditLogEntry, GuildAuditLogsRepositoryError,
    },
    state::{AppState, RequestId},
};

const DEFAULT_PAGE_SIZE: u16 = 50;
const MAX_PAGE_SIZE: u16 = 100;
const MAX_CURSOR_BYTES: usize = 512;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditLogQuery {
    cursor: Option<String>,
    limit: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CursorToken {
    created_at_nanos: i64,
    id: Uuid,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditLogData {
    entries: Vec<GuildAuditLogEntry>,
    next_cursor: Option<String>,
}

pub async fn audit_logs(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    Query(query): Query<AuditLogQuery>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let (cursor, limit) = page_parameters(&query, &request_id)?;
    let page = guild_audit_logs::list(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        cursor,
        limit,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    let next_cursor = page
        .next_cursor
        .map(encode_cursor)
        .transpose()
        .map_err(|()| invalid_projection(&request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        AuditLogData {
            entries: page.entries,
            next_cursor,
        },
    ))
}

fn page_parameters(
    query: &AuditLogQuery,
    request_id: &RequestId,
) -> Result<(Option<AuditLogCursor>, i64), ApiError> {
    let limit = query
        .limit
        .as_deref()
        .map(str::parse::<u16>)
        .transpose()
        .map_err(|_| invalid_limit(request_id))?
        .unwrap_or(DEFAULT_PAGE_SIZE);
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(invalid_limit(request_id));
    }
    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor)
        .transpose()
        .map_err(|()| invalid_cursor(request_id))?;
    Ok((cursor, i64::from(limit)))
}

fn encode_cursor(cursor: AuditLogCursor) -> Result<String, ()> {
    let created_at_nanos =
        i64::try_from(cursor.created_at.unix_timestamp_nanos()).map_err(|_| ())?;
    let payload = serde_json::to_vec(&CursorToken {
        created_at_nanos,
        id: cursor.id,
    })
    .map_err(|_| ())?;
    Ok(URL_SAFE_NO_PAD.encode(payload))
}

fn decode_cursor(value: &str) -> Result<AuditLogCursor, ()> {
    if value.is_empty() || value.len() > MAX_CURSOR_BYTES {
        return Err(());
    }
    let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| ())?;
    let token = serde_json::from_slice::<CursorToken>(&bytes).map_err(|_| ())?;
    let created_at = OffsetDateTime::from_unix_timestamp_nanos(i128::from(token.created_at_nanos))
        .map_err(|_| ())?;
    let cursor = AuditLogCursor {
        created_at,
        id: token.id,
    };
    if encode_cursor(cursor).as_deref() != Ok(value) {
        return Err(());
    }
    Ok(cursor)
}

fn invalid_limit(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_limit",
        "limit은 1 이상 100 이하이어야 합니다.",
        Some(serde_json::json!({ "min": 1, "max": MAX_PAGE_SIZE })),
    )
}

fn invalid_cursor(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_cursor",
        "cursor 형식이 올바르지 않습니다.",
        None,
    )
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

fn repository_error(source: GuildAuditLogsRepositoryError, request_id: &RequestId) -> ApiError {
    match source {
        GuildAuditLogsRepositoryError::GuildNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 길드를 찾을 수 없습니다.",
            None,
        ),
        GuildAuditLogsRepositoryError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "길드 운영자 권한이 필요합니다.",
            None,
        ),
        GuildAuditLogsRepositoryError::Database(database_error) => {
            tracing::error!(%database_error, "guild audit-log database operation failed");
            error::database_failure(&database_error, request_id)
        }
        GuildAuditLogsRepositoryError::InvariantViolation => {
            tracing::error!("guild audit-log stored invariant failed");
            invalid_projection(request_id)
        }
    }
}

fn invalid_projection(request_id: &RequestId) -> ApiError {
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
    use axum::response::IntoResponse;

    use super::*;

    #[test]
    fn opaque_cursor_roundtrips_canonically() {
        let cursor = AuditLogCursor {
            created_at: OffsetDateTime::from_unix_timestamp_nanos(1_723_456_789_123_456_000)
                .expect("valid timestamp"),
            id: Uuid::now_v7(),
        };
        let encoded = encode_cursor(cursor).expect("cursor should encode");
        assert_eq!(decode_cursor(&encoded), Ok(cursor));
        assert!(!encoded.contains('='));

        let non_canonical = URL_SAFE_NO_PAD.encode(
            format!(
                "{{\"id\":\"{}\",\"createdAtNanos\":{}}}",
                cursor.id,
                cursor.created_at.unix_timestamp_nanos()
            )
            .as_bytes(),
        );
        assert!(decode_cursor(&non_canonical).is_err());
    }

    #[test]
    fn page_parameters_reject_invalid_cursor_and_all_invalid_limits() {
        let request_id = RequestId("request-id".to_owned());
        for limit in ["", "0", "101", "-1", "text", "65536"] {
            let query = AuditLogQuery {
                cursor: None,
                limit: Some(limit.to_owned()),
            };
            assert_eq!(
                page_parameters(&query, &request_id)
                    .expect_err("invalid limit")
                    .into_response()
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
        let query = AuditLogQuery {
            cursor: Some("not-a-cursor".to_owned()),
            limit: None,
        };
        assert_eq!(
            page_parameters(&query, &request_id)
                .expect_err("invalid cursor")
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
}
