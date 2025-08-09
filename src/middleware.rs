use axum::{
    extract::MatchedPath,
    http::Request,
    middleware::Next,
    response::IntoResponse,
    body::Body,
};
use std::time::Instant;

/// Middleware to collect HTTP request metrics
pub async fn metrics_middleware(
    req: Request<Body>,
    next: Next,
) -> impl IntoResponse {
    let start = Instant::now();
    let method = req.method().to_string();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|mp| mp.as_str())
        .unwrap_or("unknown")
        .to_string();

    let response = next.run(req).await;
    let duration = start.elapsed();
    let status = response.status().as_u16();

    // Record metrics
    if let Some(metrics) = crate::metrics::METRICS.get() {
        metrics.record_http_request(&method, &path, status, duration);
    }

    response
}
