// SPDX-License-Identifier: MPL-2.0
//! HTTP routes and request logging.

use std::time::Instant;

use axum::Router;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{FromRef, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use serde_json::json;

use crate::sync::{self, Hub};
use crate::{Library, SERVICE, VERSION};

#[derive(Clone)]
struct AppState {
    library: Library,
    hub: Hub,
}

impl FromRef<AppState> for Library {
    fn from_ref(state: &AppState) -> Self {
        state.library.clone()
    }
}

impl FromRef<AppState> for Hub {
    fn from_ref(state: &AppState) -> Self {
        state.hub.clone()
    }
}

pub(crate) fn router(library: Library, hub: Hub) -> Router {
    Router::new()
        // `/healthz` too: the NAS OS greenboot checks probe both (distro/).
        .route("/health", get(health))
        .route("/healthz", get(health))
        .route(sync::PATH, get(sync_socket))
        .with_state(AppState { library, hub })
        .layer(middleware::from_fn(log_request))
}

/// Upgrades to the sync protocol (`docs/api/sync-protocol.md`). No
/// authentication yet (SRV-003, SYNC-005): the default listen address is
/// loopback.
async fn sync_socket(State(hub): State<Hub>, upgrade: WebSocketUpgrade) -> Response {
    let Some(slot) = hub.try_slot() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "sync hub is full").into_response();
    };
    upgrade
        .max_message_size(pa_sync::proto::MAX_FRAME_LEN)
        .max_frame_size(pa_sync::proto::MAX_FRAME_LEN)
        .on_upgrade(move |socket| hub.serve(socket, slot))
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
