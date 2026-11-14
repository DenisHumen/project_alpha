// SPDX-License-Identifier: MPL-2.0
//! One link: TCP → TLS 1.3 → WebSocket → `hello` + `auth` → message loop.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message as Ws;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use super::state::{Shared, Side};
use super::tls::{self, TlsSide, channel_nonce, io_err};
use crate::proto::{self, Auth, ErrorFrame, Message, ProtoRange, code, negotiate};

/// Path of the WebSocket endpoint.
const PATH: &str = "/sync";

/// Dials the hub `hub_id` at `addr` and serves the link until it breaks.
pub(crate) async fn dial(shared: Arc<Shared>, hub_id: String, addr: SocketAddr) {
    if let Err(e) = dial_inner(&shared, hub_id.clone(), addr).await {
        tracing::debug!(hub = %hub_id, error = %e, "lan sync: hub link closed");
    }
}

async fn dial_inner(shared: &Arc<Shared>, hub_id: String, addr: SocketAddr) -> io::Result<()> {
    let limit = shared.config.handshake_timeout;
    let (ws, nonces) = within(limit, async {
        let tcp = TcpStream::connect(addr).await?;
        tcp.set_nodelay(true)?;
        let stream = TlsConnector::from(Arc::clone(&shared.tls_client))
            .connect(tls::server_name(), tcp)
            .await?;
        let nonces = nonces(stream.get_ref().1, TlsSide::Client)?;
        let url = format!("wss://{}{PATH}", tls::SERVER_NAME);
        let (ws, _) = tokio_tungstenite::client_async_with_config(url, stream, Some(ws_config()))
            .await
            .map_err(io_err)?;
        Ok((ws, nonces))
    })
    .await?;
    run(shared, ws, Side::Up { hub_id }, nonces).await
}

/// Serves a link a device opened to us.
pub(crate) async fn accept(shared: Arc<Shared>, tcp: TcpStream) -> io::Result<()> {
    tcp.set_nodelay(true)?;
    let limit = shared.config.handshake_timeout;
    let (ws, nonces) = within(limit, async {
        let stream = TlsAcceptor::from(Arc::clone(&shared.tls_server))
            .accept(tcp)
            .await?;
        let nonces = nonces(stream.get_ref().1, TlsSide::Server)?;
        let ws = tokio_tungstenite::accept_async_with_config(stream, Some(ws_config()))
            .await
            .map_err(io_err)?;
        Ok((ws, nonces))
    })
    .await?;
    run(&shared, ws, Side::Down, nonces).await
}

/// Frames above the protocol limit are refused before they are buffered.
fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(proto::MAX_FRAME_LEN))
        .max_frame_size(Some(proto::MAX_FRAME_LEN))
}

async fn within<T>(limit: Duration, f: impl Future<Output = io::Result<T>>) -> io::Result<T> {
    timeout(limit, f)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "handshake timed out"))?
}

/// `(own, peer)` channel-bound nonces.
fn nonces<D>(conn: &rustls::ConnectionCommon<D>, side: TlsSide) -> io::Result<(String, String)> {
    Ok((
        channel_nonce(conn, side).map_err(io_err)?,
        channel_nonce(conn, side.peer()).map_err(io_err)?,
    ))
}

/// Removes the link from the node however the task ends (also on abort).
struct Registered<'a> {
    shared: &'a Shared,
    id: u64,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        self.shared.lock().unregister(self.id);
    }
}

fn reject(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, text.to_owned())
}

async fn run<S>(
    shared: &Arc<Shared>,
    ws: WebSocketStream<S>,
    side: Side,
    (own_nonce, peer_nonce): (String, String),
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Option<String>>();
    let ping = shared.config.ping_interval;
    let writer = tokio::spawn(async move {
        let mut tick = tokio::time::interval(ping);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let out = tokio::select! {
                frame = rx.recv() => match frame {
                    Some(Some(text)) => Ws::text(text),
                    Some(None) | None => break,
                },
                _ = tick.tick() => Ws::Ping(Default::default()),
            };
            if sink.send(out).await.is_err() {
                return;
            }
        }
        let _ = sink.close().await;
    });
    let _writer = super::AbortOnDrop(writer);
    let send = |msg: &Message| {
        if let Ok(text) = proto::encode(msg) {
            let _ = tx.send(Some(text));
        }
    };
    let fail = |code: &str| {
        send(&Message::Error(ErrorFrame {
            code: code.to_owned(),
            message: None,
            op_id: None,
        }));
        let _ = tx.send(None);
    };

    // Handshake: hello both ways, then auth both ways, all within one
    // deadline (a peer cannot stall it with junk frames).
    let limit = shared.config.handshake_timeout;
    let deadline = tokio::time::Instant::now() + limit;
    let mut first = async || {
        tokio::time::timeout_at(deadline, next(&mut stream, limit))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "handshake timed out"))?
    };
    send(&Message::Hello(shared.lock().hello(&side, own_nonce.clone())));
    let hello = match first().await? {
        (_, Message::Hello(h)) => h,
        _ => {
            fail(code::HANDSHAKE);
            return Err(reject("expected hello"));
        }
    };
    if negotiate(ProtoRange::LOCAL, hello.proto).is_err() {
        fail(code::PROTO_UNSUPPORTED);
        return Err(reject("no common protocol version"));
    }
    let wrong_hub = matches!(&side, Side::Up { hub_id } if *hub_id != hello.device_id);
    if wrong_hub || hello.nonce.as_deref() != Some(peer_nonce.as_str()) {
        fail(code::UNAUTHORIZED);
        return Err(reject("unexpected peer or unbound nonce"));
    }
    let sig = {
        let state = shared.lock();
        let identity = state.family.identity();
        pa_pairing::auth::prove(identity, &hello.device_id, &own_nonce, &peer_nonce)
    };
    send(&Message::Auth(Auth { sig }));
    let authorized = match first().await? {
        (_, Message::Auth(a)) => shared
            .lock()
            .verify(&hello.device_id, &peer_nonce, &own_nonce, &a.sig)
            .is_ok(),
        _ => false,
    };
    if !authorized {
        fail(code::UNAUTHORIZED);
        return Err(reject("peer is not a family member"));
    }

    drop(first);
    let id = shared.lock().register(side, &hello, tx.clone());
    let _registered = Registered { shared, id };
    let dead_after = shared.config.dead_after;
    loop {
        let (raw, msg) = next(&mut stream, dead_after).await?;
        shared.lock().on_message(id, &raw, msg);
    }
}

/// The next protocol message; pings and pongs count as liveness. Frames that
/// do not decode and unknown message types are skipped.
async fn next<S>(
    stream: &mut futures_util::stream::SplitStream<WebSocketStream<S>>,
    limit: Duration,
) -> io::Result<(String, Message)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let frame = timeout(limit, stream.next())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "link is silent"))?
            .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?
            .map_err(io_err)?;
        match frame {
            Ws::Text(text) => match proto::decode(text.as_str()) {
                Ok(Message::Unknown) | Err(_) => {}
                Ok(msg) => return Ok((text.as_str().to_owned(), msg)),
            },
            Ws::Close(_) => return Err(io::Error::from(io::ErrorKind::ConnectionAborted)),
            _ => {}
        }
    }
}
