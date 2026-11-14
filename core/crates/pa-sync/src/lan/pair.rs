// SPDX-License-Identifier: MPL-2.0
//! Pairing over the LAN (UI-019): the [`pa_pairing::pair`] messages over
//! WebSocket on TLS 1.3, between two apps without a server.
//!
//! The inviter runs a [`PairHost`] while it shows its code: a listener on
//! its own port that accepts one attempt at a time. The joiner dials it with
//! [`join`] — the address comes from the QR code (`a=`) or from the
//! `_alpha-pair._tcp` announcement of the inviter. TLS uses an ephemeral
//! certificate and accepts any certificate on the joiner's side: what
//! authenticates both ends is SPAKE2 over the code (and the inviter's key
//! from the QR code), not TLS. TLS only keeps the code-derived messages off
//! the air for passive listeners.

use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pa_pairing::pair::{Invitation, Inviter, JoinRequest, Joined, Joiner, PairMessage, code};
use pa_pairing::pair::PairingCode;
use pa_pairing::{DeviceIdentity, DeviceKind};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message as Ws;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use super::AbortOnDrop;
use super::tls::{self, io_err};

/// Path of the pairing WebSocket endpoint.
const PATH: &str = "/pair";
/// Longest wait for one pairing message.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(15);

/// Admits the joiner into the family: runs [`Inviter::welcome`] on the
/// inviter's family, saves it and returns the `pair.welcome` to send.
pub type Admit =
    Arc<dyn Fn(Inviter, &JoinRequest) -> Result<PairMessage, pa_pairing::Error> + Send + Sync>;

/// What happened to an attempt on a [`PairHost`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// A device joined the family.
    Paired { device_id: String, name: String },
    /// An attempt failed (wrong code, a broken connection); the invitation
    /// stays open while it has attempts left.
    Failed { reason: String },
    /// The invitation expired or ran out of attempts.
    Closed,
}

/// The inviter's listener for one invitation. Dropping it stops listening.
pub struct PairHost {
    local_addr: SocketAddr,
    invitation: Arc<Mutex<Option<Invitation>>>,
    _task: AbortOnDrop,
}

fn now_ms() -> u64 {
    crate::hlc::system_ms()
}

impl PairHost {
    /// Listens on `bind` for joiners of `invitation`. [`HostEvent`]s go to
    /// the returned receiver; after [`HostEvent::Paired`] the host closes.
    pub async fn start(
        bind: SocketAddr,
        invitation: Invitation,
        admit: Admit,
    ) -> io::Result<(Self, mpsc::UnboundedReceiver<HostEvent>)> {
        let listener = TcpListener::bind(bind).await?;
        let local_addr = listener.local_addr()?;
        let acceptor = TlsAcceptor::from(Arc::new(tls::server_config()?));
        let invitation = Arc::new(Mutex::new(Some(invitation)));
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(serve(listener, acceptor, Arc::clone(&invitation), admit, tx));
        Ok((
            Self {
                local_addr,
                invitation,
                _task: AbortOnDrop(task),
            },
            rx,
        ))
    }

    /// Listening address.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Whether the invitation still takes attempts.
    pub fn is_open(&self) -> bool {
        lock(&self.invitation)
            .as_ref()
            .is_some_and(|i| i.is_open(now_ms()))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn serve(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    invitation: Arc<Mutex<Option<Invitation>>>,
    admit: Admit,
    events: mpsc::UnboundedSender<HostEvent>,
) {
    let expiry = lock(&invitation)
        .as_ref()
        .map_or(0, Invitation::expires_ms)
        .saturating_sub(now_ms());
    let deadline = tokio::time::Instant::now() + Duration::from_millis(expiry);
    loop {
        let accepted = tokio::select! {
            r = listener.accept() => r,
            () = tokio::time::sleep_until(deadline) => {
                *lock(&invitation) = None;
                let _ = events.send(HostEvent::Closed);
                return;
            }
        };
        let Ok((tcp, _)) = accepted else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        // One attempt at a time: SPAKE2 gives an attacker one guess per
        // attempt, and the invitation counts them.
        let event = match attempt(&acceptor, tcp, &invitation, &admit).await {
            Ok((device_id, name)) => {
                *lock(&invitation) = None;
                let _ = events.send(HostEvent::Paired { device_id, name });
                return;
            }
            Err(reason) => HostEvent::Failed { reason },
        };
        let _ = events.send(event);
        if !lock(&invitation)
            .as_ref()
            .is_some_and(|i| i.is_open(now_ms()))
        {
            *lock(&invitation) = None;
            let _ = events.send(HostEvent::Closed);
            return;
        }
    }
}

fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(pa_pairing::pair::MAX_FRAME_LEN))
        .max_frame_size(Some(pa_pairing::pair::MAX_FRAME_LEN))
}

async fn attempt(
    acceptor: &TlsAcceptor,
    tcp: TcpStream,
    invitation: &Mutex<Option<Invitation>>,
    admit: &Admit,
) -> Result<(String, String), String> {
    let mut ws = timeout(STEP_TIMEOUT, async {
        tcp.set_nodelay(true)?;
        let stream = acceptor.accept(tcp).await?;
        tokio_tungstenite::accept_async_with_config(stream, Some(ws_config()))
            .await
            .map_err(io_err)
    })
    .await
    .map_err(|_| "handshake timed out".to_owned())?
    .map_err(|e| e.to_string())?;
    let result = invite(&mut ws, invitation, admit).await;
    let _ = ws.send(Ws::Close(None)).await;
    result
}

async fn invite<S>(
    ws: &mut WebSocketStream<S>,
    invitation: &Mutex<Option<Invitation>>,
    admit: &Admit,
) -> Result<(String, String), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let started = lock(invitation)
        .as_mut()
        .map(|inv| Inviter::start(inv, now_ms()));
    let (mut inviter, hello) = match started {
        Some(Ok(started)) => started,
        _ => {
            send(ws, &PairMessage::error(code::EXPIRED, None)).await;
            return Err("invitation closed".into());
        }
    };
    send(ws, &hello).await;
    let msg = recv(ws).await.ok_or("no hello")?;
    if inviter.receive_hello(&msg).is_err() {
        send(ws, &PairMessage::error(code::BAD_MESSAGE, None)).await;
        return Err("bad hello".into());
    }
    let msg = recv(ws).await.ok_or("no intro")?;
    let request = match inviter.open_intro(&msg) {
        Ok(request) => request,
        Err(pa_pairing::Error::WrongCode) => {
            send(ws, &PairMessage::error(code::WRONG_CODE, None)).await;
            return Err("wrong code".into());
        }
        Err(_) => {
            send(ws, &PairMessage::error(code::BAD_MESSAGE, None)).await;
            return Err("bad intro".into());
        }
    };
    let welcome = match admit(inviter, &request) {
        Ok(welcome) => welcome,
        Err(e) => {
            send(ws, &PairMessage::error(code::DENIED, None)).await;
            return Err(format!("refused: {e}"));
        }
    };
    send(ws, &welcome).await;
    Ok((request.device_id, request.name))
}

async fn send<S>(ws: &mut WebSocketStream<S>, msg: &PairMessage)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let _ = ws.send(Ws::Text(msg.encode().into())).await;
}

async fn recv<S>(ws: &mut WebSocketStream<S>) -> Option<PairMessage>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let msg = timeout(STEP_TIMEOUT, ws.next()).await.ok()??.ok()?;
        match msg {
            Ws::Text(text) => return PairMessage::decode(text.as_str()).ok(),
            Ws::Close(_) => return None,
            _ => {}
        }
    }
}

/// Why [`join`] failed.
#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    /// No connection to the inviter.
    #[error("inviter unreachable: {0}")]
    Unreachable(String),
    /// The pairing itself failed (wrong code, expired invitation, refused,
    /// a key that does not match the QR code).
    #[error(transparent)]
    Pairing(#[from] pa_pairing::Error),
    /// The connection broke in the middle.
    #[error("pairing interrupted")]
    Interrupted,
}

/// What the joiner says about itself.
#[derive(Debug, Clone)]
pub struct JoinAs<'a> {
    pub identity: &'a DeviceIdentity,
    pub name: &'a str,
    pub kind: DeviceKind,
}

/// Pairs with the inviter at `addr`: types `code` (and checks the
/// inviter's key from the QR code, if given). Returns the family to save.
pub async fn join(
    addr: SocketAddr,
    me: JoinAs<'_>,
    pairing_code: &PairingCode,
    inviter: Option<[u8; 32]>,
) -> Result<Joined, JoinError> {
    let unreachable = |e: io::Error| JoinError::Unreachable(e.to_string());
    let mut ws = timeout(STEP_TIMEOUT, async {
        let tcp = TcpStream::connect(addr).await?;
        tcp.set_nodelay(true)?;
        let stream = TlsConnector::from(Arc::new(tls::client_config()?))
            .connect(tls::server_name(), tcp)
            .await?;
        let url = format!("wss://{}{PATH}", tls::SERVER_NAME);
        let (ws, _) = tokio_tungstenite::client_async_with_config(url, stream, Some(ws_config()))
            .await
            .map_err(io_err)?;
        Ok::<_, io::Error>(ws)
    })
    .await
    .map_err(|_| JoinError::Unreachable("timed out".into()))?
    .map_err(unreachable)?;

    let theirs = recv(&mut ws).await.ok_or(JoinError::Interrupted)?;
    if let PairMessage::Error { code, .. } = &theirs {
        return Err(JoinError::Pairing(refused(code)));
    }
    let (mut joiner, hello) = Joiner::start(me.identity, me.name, me.kind, pairing_code, inviter)?;
    send(&mut ws, &hello).await;
    let intro = joiner.intro(me.identity, &theirs)?;
    send(&mut ws, &intro).await;
    let answer = recv(&mut ws).await.ok_or(JoinError::Interrupted)?;
    let joined = joiner.finish(&answer)?;
    let _ = ws.send(Ws::Close(None)).await;
    Ok(joined)
}

fn refused(c: &str) -> pa_pairing::Error {
    match c {
        code::WRONG_CODE => pa_pairing::Error::WrongCode,
        code::EXPIRED => pa_pairing::Error::Expired,
        other => pa_pairing::Error::Refused(other.to_owned()),
    }
}
