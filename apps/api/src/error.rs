use axum::{
    Json,
    http::{HeaderMap, HeaderName, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::Value;

use crate::state::RequestId;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SuccessEnvelope<T> {
    data: T,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResponseMeta {
    request_id: String,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ErrorCode(&'static str);

impl ErrorCode {
    pub(crate) const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    request_id: String,
    details: Option<Value>,
}

impl ApiError {
    pub fn new(
        status: StatusCode,
        request_id: &RequestId,
        code: &'static str,
        message: &'static str,
        details: Option<Value>,
    ) -> Self {
        Self {
            status,
            code,
            message,
            request_id: request_id.0.clone(),
            details,
        }
    }

    fn from_status(status: StatusCode, request_id: &RequestId) -> Self {
        let (code, message) = match status {
            StatusCode::BAD_REQUEST => ("bad_request", "요청 형식이 올바르지 않습니다."),
            StatusCode::UNAUTHORIZED => ("unauthorized", "인증이 필요합니다."),
            StatusCode::FORBIDDEN => ("forbidden", "요청을 수행할 권한이 없습니다."),
            StatusCode::NOT_FOUND => ("not_found", "요청한 API 경로를 찾을 수 없습니다."),
            StatusCode::METHOD_NOT_ALLOWED => (
                "method_not_allowed",
                "이 API 경로에서 지원하지 않는 HTTP 메서드입니다.",
            ),
            StatusCode::REQUEST_TIMEOUT => ("request_timeout", "요청 처리 시간이 초과되었습니다."),
            StatusCode::CONFLICT => ("conflict", "현재 상태와 충돌하는 요청입니다."),
            StatusCode::UNSUPPORTED_MEDIA_TYPE => (
                "unsupported_media_type",
                "지원하지 않는 요청 본문 형식입니다.",
            ),
            StatusCode::UNPROCESSABLE_ENTITY => (
                "unprocessable_entity",
                "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
            ),
            StatusCode::TOO_MANY_REQUESTS => (
                "rate_limited",
                "요청이 너무 많습니다. 잠시 후 다시 시도해 주세요.",
            ),
            StatusCode::SERVICE_UNAVAILABLE => (
                "service_unavailable",
                "서비스를 일시적으로 사용할 수 없습니다.",
            ),
            status if status.is_client_error() => ("client_error", "요청을 처리할 수 없습니다."),
            _ => (
                "internal_error",
                "요청을 처리하는 중 내부 오류가 발생했습니다.",
            ),
        };

        Self::new(status, request_id, code, message, None)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                    request_id: self.request_id,
                    details: self.details,
                },
            }),
        )
            .into_response();
        response.extensions_mut().insert(ErrorCode(self.code));
        response
    }
}

pub fn success<T: Serialize>(status: StatusCode, request_id: &RequestId, data: T) -> Response {
    (
        status,
        Json(SuccessEnvelope {
            data,
            meta: ResponseMeta {
                request_id: request_id.0.clone(),
            },
        }),
    )
        .into_response()
}

pub fn failure(
    status: StatusCode,
    request_id: &RequestId,
    code: &'static str,
    message: &'static str,
    details: Option<Value>,
) -> Response {
    ApiError::new(status, request_id, code, message, details).into_response()
}

pub(crate) fn normalize_error_response(
    response: Response,
    request_id: &RequestId,
    max_body_bytes: usize,
) -> Response {
    if response.extensions().get::<ErrorCode>().is_some()
        || !(response.status().is_client_error() || response.status().is_server_error())
    {
        return response;
    }

    let status = response.status();
    let preserved_headers = preserved_protocol_headers(response.headers());
    let mut normalized = if status == StatusCode::PAYLOAD_TOO_LARGE {
        payload_too_large(request_id, max_body_bytes)
    } else {
        ApiError::from_status(status, request_id).into_response()
    };

    for (name, value) in preserved_headers {
        normalized.headers_mut().append(name, value);
    }
    normalized
}

fn preserved_protocol_headers(headers: &HeaderMap) -> Vec<(HeaderName, axum::http::HeaderValue)> {
    [header::ALLOW, header::WWW_AUTHENTICATE, header::RETRY_AFTER]
        .into_iter()
        .flat_map(|name| {
            headers
                .get_all(&name)
                .iter()
                .cloned()
                .map(move |value| (name.clone(), value))
        })
        .collect()
}

pub fn not_found(request_id: &RequestId) -> Response {
    failure(
        StatusCode::NOT_FOUND,
        request_id,
        "not_found",
        "요청한 API 경로를 찾을 수 없습니다.",
        None,
    )
}

pub fn method_not_allowed(request_id: &RequestId) -> Response {
    failure(
        StatusCode::METHOD_NOT_ALLOWED,
        request_id,
        "method_not_allowed",
        "이 API 경로에서 지원하지 않는 HTTP 메서드입니다.",
        None,
    )
}

pub fn payload_too_large(request_id: &RequestId, max_body_bytes: usize) -> Response {
    failure(
        StatusCode::PAYLOAD_TOO_LARGE,
        request_id,
        "payload_too_large",
        "요청 본문이 허용된 크기를 초과했습니다.",
        Some(serde_json::json!({ "maxBodyBytes": max_body_bytes })),
    )
}

pub fn request_timeout(request_id: &RequestId) -> Response {
    failure(
        StatusCode::SERVICE_UNAVAILABLE,
        request_id,
        "request_timeout",
        "요청 처리 시간이 초과되었습니다.",
        None,
    )
}

pub fn internal(request_id: &RequestId) -> Response {
    failure(
        StatusCode::INTERNAL_SERVER_ERROR,
        request_id,
        "internal_error",
        "요청을 처리하는 중 내부 오류가 발생했습니다.",
        None,
    )
}

pub fn database_failure(error: &sqlx::Error, request_id: &RequestId) -> ApiError {
    if is_transient_database_error(error) {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_unavailable",
            "데이터베이스를 일시적으로 사용할 수 없습니다.",
            None,
        )
    } else {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            request_id,
            "internal_error",
            "요청을 처리하는 중 내부 오류가 발생했습니다.",
            None,
        )
    }
}

fn is_transient_database_error(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed
        | sqlx::Error::BeginFailed => true,
        sqlx::Error::Database(error) => error.code().is_some_and(|code| {
            code.starts_with("08")
                || code.starts_with("53")
                || matches!(
                    code.as_ref(),
                    "40001" | "40P01" | "55P03" | "57014" | "57P01" | "57P02" | "57P03"
                )
        }),
        _ => false,
    }
}

#[cfg(test)]
mod database_tests {
    use super::*;

    #[test]
    fn temporary_pool_failures_are_service_unavailable() {
        let request_id = RequestId("request-id".to_owned());

        assert_eq!(
            database_failure(&sqlx::Error::PoolTimedOut, &request_id).status,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            database_failure(&sqlx::Error::RowNotFound, &request_id).status,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
