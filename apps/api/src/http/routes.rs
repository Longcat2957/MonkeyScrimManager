use axum::{Extension, extract::State, http::StatusCode, response::Response};
use serde::Serialize;

use crate::{
    error,
    state::{AppState, RequestId},
};

#[derive(Serialize)]
struct LivenessData {
    status: &'static str,
}

pub async fn healthz(Extension(request_id): Extension<RequestId>) -> Response {
    error::success(StatusCode::OK, &request_id, LivenessData { status: "ok" })
}

pub async fn readyz(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Response {
    let report = state.readiness().report().await;
    if report.is_ready() {
        error::success(StatusCode::OK, &request_id, report)
    } else {
        let details = serde_json::to_value(&report).ok();
        error::failure(
            StatusCode::SERVICE_UNAVAILABLE,
            &request_id,
            "service_not_ready",
            "서비스가 요청을 받을 준비가 되지 않았습니다.",
            details,
        )
    }
}

pub async fn not_found(Extension(request_id): Extension<RequestId>) -> Response {
    error::not_found(&request_id)
}

pub async fn method_not_allowed(Extension(request_id): Extension<RequestId>) -> Response {
    error::method_not_allowed(&request_id)
}
