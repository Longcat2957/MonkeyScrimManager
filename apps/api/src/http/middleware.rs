use std::{panic::AssertUnwindSafe, time::Instant};

use axum::{
    extract::{MatchedPath, Request, State},
    http::{HeaderName, HeaderValue, header},
    middleware::Next,
    response::Response,
};
use futures_util::FutureExt;
use tracing::{Instrument, info, info_span};

use crate::{error, state::AppState};

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");
const CONTENT_TYPE_OPTIONS: HeaderName = HeaderName::from_static("x-content-type-options");
const FRAME_OPTIONS: HeaderName = HeaderName::from_static("x-frame-options");
const REFERRER_POLICY: HeaderName = HeaderName::from_static("referrer-policy");

pub async fn request_pipeline(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let started_at = Instant::now();
    let request_id = state.next_request_id();
    let method = request.method().clone();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "<unmatched>".to_owned(), |path| path.as_str().to_owned());
    request.extensions_mut().insert(request_id.clone());

    let span = info_span!(
        "http.request",
        request_id = %request_id.0,
        trace_id = %request_id.0,
        method = %method,
        route,
    );
    let next_request = AssertUnwindSafe(next.run(request)).catch_unwind();
    let outcome = tokio::time::timeout(state.config().request_timeout(), next_request)
        .instrument(span.clone())
        .await;

    let response = match outcome {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => {
            tracing::error!(parent: &span, "request handler panicked");
            error::internal(&request_id)
        }
        Err(_) => error::request_timeout(&request_id),
    };
    let mut response =
        error::normalize_error_response(response, &request_id, state.config().max_body_bytes());

    apply_response_headers(&mut response, &request_id.0);
    let error_code = response
        .extensions()
        .get::<error::ErrorCode>()
        .map(|code| code.as_str())
        .unwrap_or_default();
    info!(
        parent: &span,
        status = response.status().as_u16(),
        latency_ms = started_at.elapsed().as_millis(),
        error_code,
        "request completed"
    );
    response
}

fn apply_response_headers(response: &mut Response, request_id: &str) {
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(request_id) {
        headers.insert(REQUEST_ID_HEADER, value);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
}
