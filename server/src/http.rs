// SPDX-License-Identifier: MPL-2.0
//! HTTP routes and request logging.

use std::time::Instant;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use serde_json::json;

use crate::{Library, SERVICE, VERSION};

pub(crate) fn router(library: Library) -> Router {
    Router::new()
        // `/healthz` too: the NAS OS greenboot checks probe both (distro/).
        .route("/health", get(health))
        .route("/healthz", get(health))
        .with_state(library)
        .layer(middleware::from_fn(log_request))
}

/// Liveness and readiness: 200 if the library database answers, 503 if not.
async fn health(State(library): State<Library>) -> Response {
    match library.schema_version().await {
        Ok(schema) => Json(json!({
            "service": SERVICE,
            "status": "ok",
            "version": VERSION,
            "db": { "status": "ok", "schema_version": schema },
        }))
        .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "health: library database unavailable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "service": SERVICE,
                    "status": "error",
                    "version": VERSION,
                    "db": { "status": "error" },
                })),
            )
                .into_response()
        }
    }
}

/// One structured log line per request. Query strings are not logged: later
/// endpoints may carry tokens there.
async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();
    let response = next.run(request).await;
    tracing::info!(
        %method,
        path,
        status = response.status().as_u16(),
        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
        "request"
    );
    response
}
