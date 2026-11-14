// SPDX-License-Identifier: MPL-2.0
//! HTTP routes and request logging.

use std::time::Instant;

use std::net::SocketAddr;

use axum::Router;
use axum::extract::connect_info::Connected;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, FromRef, Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{delete, get, post};
use axum::serve::IncomingStream;
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;

use crate::tls::TlsListener;

use crate::pairing::{self, Pairing};
use crate::sync::{self, Hub};
use crate::{Library, SERVICE, VERSION};

/// Remote address of a connection, for plain TCP and TLS alike (axum only
/// implements `Connected` for `SocketAddr` over `TcpListener`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct PeerAddr(pub(crate) SocketAddr);

impl Connected<IncomingStream<'_, TcpListener>> for PeerAddr {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

impl Connected<IncomingStream<'_, TlsListener>> for PeerAddr {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

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

impl FromRef<AppState> for Pairing {
    fn from_ref(state: &AppState) -> Self {
        state.hub.pairing().clone()
    }
}

pub(crate) fn router(library: Library, hub: Hub) -> Router {
    Router::new()
        // `/healthz` too: the NAS OS greenboot checks probe both (distro/).
        .route("/health", get(health))
        .route("/healthz", get(health))
        .route(sync::PATH, get(sync_socket))
        .route(pairing::PAIR_PATH, get(pair_socket))
        .route(INVITE_PATH, post(invite))
        .route(DEVICES_PATH, get(devices))
        .route(&format!("{DEVICES_PATH}/{{id}}"), delete(revoke))
        .with_state(AppState { library, hub })
        .layer(middleware::from_fn(log_request))
}

/// Opens a pairing invitation (loopback only).
const INVITE_PATH: &str = "/api/v1/pair/invite";
/// Device list and revocation (loopback only).
const DEVICES_PATH: &str = "/api/v1/devices";

/// Administration of the family is allowed from this machine only, until
/// user accounts exist (SRV-003). Returns the refusal for other peers.
fn local_only(peer: SocketAddr) -> Option<Response> {
    if peer.ip().is_loopback() {
        None
    } else {
        Some((StatusCode::FORBIDDEN, "allowed from the server itself only").into_response())
    }
}

#[derive(Deserialize)]
struct InviteQuery {
    /// `qr`: a long code for a QR code only; otherwise 6 digits.
    #[serde(default)]
    kind: Option<String>,
}

/// `POST /api/v1/pair/invite[?kind=qr]`: a new pairing code and its QR
/// payload, replacing an open invitation.
async fn invite(
    ConnectInfo(PeerAddr(peer)): ConnectInfo<PeerAddr>,
    State(pairing): State<Pairing>,
    Query(query): Query<InviteQuery>,
) -> Response {
    if let Some(denied) = local_only(peer) {
        return denied;
    }
    match pairing.invite(query.kind.as_deref() == Some("qr")) {
        Ok(invite) => Json(json!({
            "code": invite.code,
            "qr": invite.qr,
            "expires_ms": invite.expires_ms,
            "fingerprint": pairing.fingerprint(),
        }))
        .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "pairing: no invitation");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /api/v1/devices`: the devices of the family, revoked ones too.
async fn devices(
    ConnectInfo(PeerAddr(peer)): ConnectInfo<PeerAddr>,
    State(pairing): State<Pairing>,
) -> Response {
    if let Some(denied) = local_only(peer) {
        return denied;
    }
    Json(json!({
        "family": pairing.family_id(),
        "devices": pairing.devices(),
    }))
    .into_response()
}

/// `DELETE /api/v1/devices/{id}`: revokes a device and rotates the family
/// key.
async fn revoke(
    ConnectInfo(PeerAddr(peer)): ConnectInfo<PeerAddr>,
    State(pairing): State<Pairing>,
    Path(id): Path<String>,
) -> Response {
    if let Some(denied) = local_only(peer) {
        return denied;
    }
    let result = tokio::task::spawn_blocking(move || pairing.revoke(&id))
        .await
        .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(pa_pairing::Error::Unknown(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(pa_pairing::Error::Revoked(_)) => StatusCode::GONE.into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

/// Upgrades to pairing (`pa_pairing::pair`): the server is the inviter of
/// the open invitation.
async fn pair_socket(State(pairing): State<Pairing>, upgrade: WebSocketUpgrade) -> Response {
    upgrade
        .max_message_size(pa_pairing::pair::MAX_FRAME_LEN)
        .max_frame_size(pa_pairing::pair::MAX_FRAME_LEN)
        .on_upgrade(move |socket| pairing.serve(socket))
}

/// Upgrades to the sync protocol (`docs/api/sync-protocol.md`). Devices
/// authenticate with their paired keys (SYNC-005).
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
