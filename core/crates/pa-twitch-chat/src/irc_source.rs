// SPDX-License-Identifier: MPL-2.0
//! Anonymous read-only Twitch IRC over WebSocket (`justinfan<random>`).

use std::sync::Arc;
use std::time::Duration;

use pa_twitch::{BoxFuture, Clock, SentChatMessage};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use crate::irc::{IrcInput, interpret, parse_frame};
use crate::model::{ChatEvent, SourceStatus};
use crate::source::{Backoff, ChatError, ChatSource, SourceKind};
use crate::ws::{WsConnection, WsConnector};

/// Twitch IRC over secure WebSocket (plain `ws://` was shut down in 2025).
pub const DEFAULT_IRC_URL: &str = "wss://irc-ws.chat.twitch.tv:443";

/// Rooms one anonymous connection may join (Twitch limit is 100 per
/// account; keep a margin).
pub const MAX_IRC_CHANNELS: usize = 90;

/// Configuration of [`IrcChatSource`].
#[derive(Debug, Clone)]
pub struct IrcConfig {
    /// WebSocket URL.
    pub url: String,
    /// Channel logins to join (multi-channel mode).
    pub channels: Vec<String>,
    /// Nick; `None` picks `justinfan<random>`.
    pub nick: Option<String>,
    /// Reconnect backoff.
    pub backoff: Backoff,
    /// Reconnect when nothing arrives for this long (Twitch sends `PING`
    /// about every 5 minutes).
    pub idle_timeout: Duration,
    /// Give up after this many failed attempts in a row (`None`: never).
    pub max_attempts: Option<u32>,
    /// Events buffered for a slow consumer before the socket is paused.
    pub buffer: usize,
}

impl IrcConfig {
    /// Defaults for the given channel logins.
    pub fn new(channels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            url: DEFAULT_IRC_URL.to_owned(),
            channels: channels.into_iter().map(Into::into).collect(),
            nick: None,
            backoff: Backoff::default(),
            idle_timeout: Duration::from_secs(6 * 60),
            max_attempts: None,
            buffer: 4096,
        }
    }
}

/// Validates and lower-cases a channel login (`[a-z0-9_]{1,25}`).
pub fn normalise_login(login: &str) -> Result<String, ChatError> {
    let login = login.trim().trim_start_matches('#').to_lowercase();
    if login.is_empty()
        || login.len() > 25
        || !login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(ChatError::InvalidArgument(
            "channel login must be 1-25 characters [a-z0-9_]",
        ));
    }
    Ok(login)
}

/// Anonymous nick `justinfan<random>`; Twitch accepts any number.
pub fn anonymous_nick() -> String {
    let mut b = [0u8; 4];
    let n = if getrandom::fill(&mut b).is_ok() {
        10_000 + u32::from_le_bytes(b) % 89_990_000
    } else {
        12_345
    };
    format!("justinfan{n}")
}

/// Read-only chat without sign-in. Must be created inside a Tokio runtime.
pub struct IrcChatSource {
    events: Mutex<mpsc::Receiver<ChatEvent>>,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for IrcChatSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrcChatSource").finish_non_exhaustive()
    }
}

impl IrcChatSource {
    /// Starts the connection loop.
    pub fn spawn(
        config: IrcConfig,
        connector: Arc<dyn WsConnector>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, ChatError> {
        if config.channels.is_empty() || config.channels.len() > MAX_IRC_CHANNELS {
            return Err(ChatError::InvalidArgument("join 1-90 channels"));
        }
        let channels = config
            .channels
            .iter()
            .map(|c| normalise_login(c))
            .collect::<Result<Vec<_>, _>>()?;
        let nick = config.nick.clone().unwrap_or_else(anonymous_nick);
        let (tx, rx) = mpsc::channel(config.buffer.max(1));
        let driver = Driver {
            config,
            channels,
            nick,
            connector,
            clock,
            tx,
        };
        Ok(Self {
            events: Mutex::new(rx),
            task: tokio::spawn(driver.run()),
        })
    }

    /// Starts with the real `wss://` transport and system clock.
    #[cfg(feature = "ws")]
    pub fn connect(config: IrcConfig) -> Result<Self, ChatError> {
        Self::spawn(
            config,
            Arc::new(crate::ws::TungsteniteConnector::new()?),
            Arc::new(pa_twitch::SystemClock),
        )
    }
}

impl Drop for IrcChatSource {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ChatSource for IrcChatSource {
    fn kind(&self) -> SourceKind {
        SourceKind::AnonymousIrc
    }

    fn can_send(&self) -> bool {
        false
    }

    fn next_event(&self) -> BoxFuture<'_, Option<ChatEvent>> {
        Box::pin(async move { self.events.lock().await.recv().await })
    }

    fn send_message<'a>(
        &'a self,
        _broadcaster_id: &'a str,
        _text: &'a str,
        _reply_to: Option<&'a str>,
    ) -> BoxFuture<'a, Result<SentChatMessage, ChatError>> {
        Box::pin(async { Err(ChatError::ReadOnly) })
    }

    fn close(&self) {
        self.task.abort();
    }
}

/// How a session ended.
enum End {
    /// The consumer dropped the source.
    ConsumerGone,
    /// `RECONNECT`: move at once.
    Reconnect,
    /// Lost; reconnect with backoff. `healthy` = the session got as far as
    /// the welcome, so the attempt counter restarts.
    Lost { reason: String, healthy: bool },
}

struct Driver {
    config: IrcConfig,
    channels: Vec<String>,
    nick: String,
    connector: Arc<dyn WsConnector>,
    clock: Arc<dyn Clock>,
    tx: mpsc::Sender<ChatEvent>,
}

impl Driver {
    async fn emit(&self, event: ChatEvent) -> bool {
        self.tx.send(event).await.is_ok()
    }

    async fn run(self) {
        let mut attempt: u32 = 0;
        loop {
            let end = match self.connector.connect(&self.config.url).await {
                Ok(conn) => self.session(conn).await,
                Err(e) => End::Lost {
                    reason: e.to_string(),
                    healthy: false,
                },
            };
            let (delay, reason) = match end {
                End::ConsumerGone => return,
                End::Reconnect => {
                    attempt = 1;
                    (Duration::ZERO, "server asked to reconnect".to_owned())
                }
                End::Lost { reason, healthy } => {
                    attempt = if healthy { 1 } else { attempt + 1 };
                    if self.config.max_attempts.is_some_and(|max| attempt > max) {
                        let _ = self
                            .emit(ChatEvent::Status(SourceStatus::Stopped { reason }))
                            .await;
                        return;
                    }
                    (self.config.backoff.delay(attempt), reason)
                }
            };
            let status = SourceStatus::Reconnecting {
                attempt,
                delay,
                reason,
            };
            if !self.emit(ChatEvent::Status(status)).await {
                return;
            }
            if !delay.is_zero() {
                self.clock.sleep(delay).await;
            }
        }
    }

    async fn session(&self, mut conn: Box<dyn WsConnection>) -> End {
        let end = self.session_inner(conn.as_mut()).await;
        conn.close().await;
        end
    }

    async fn session_inner(&self, conn: &mut dyn WsConnection) -> End {
        let lost = |e: ChatError, healthy| End::Lost {
            reason: e.to_string(),
            healthy,
        };
        let hello = [
            "CAP REQ :twitch.tv/tags twitch.tv/commands".to_owned(),
            // Any password works for justinfan; this is the customary one.
            "PASS SCHMOOPIIE".to_owned(),
            format!("NICK {}", self.nick),
        ];
        for line in hello {
            if let Err(e) = conn.send_text(line).await {
                return lost(e, false);
            }
        }
        let mut welcomed = false;
        loop {
            let frame = tokio::select! {
                biased;
                frame = conn.recv_text() => frame,
                () = self.clock.sleep(self.config.idle_timeout) => {
                    return End::Lost { reason: "no data from Twitch (idle timeout)".to_owned(), healthy: welcomed };
                }
            };
            let frame = match frame {
                Some(Ok(f)) => f,
                Some(Err(e)) => return lost(e, welcomed),
                None => {
                    return End::Lost {
                        reason: "connection closed by Twitch".to_owned(),
                        healthy: welcomed,
                    };
                }
            };
            for msg in parse_frame(&frame) {
                match interpret(&msg, &self.nick) {
                    IrcInput::Event(event) => {
                        if !self.emit(event).await {
                            return End::ConsumerGone;
                        }
                    }
                    IrcInput::Ping(payload) => {
                        if let Err(e) = conn.send_text(format!("PONG :{payload}")).await {
                            return lost(e, welcomed);
                        }
                    }
                    IrcInput::Reconnect => return End::Reconnect,
                    IrcInput::Welcome => {
                        welcomed = true;
                        if !self.emit(ChatEvent::Status(SourceStatus::Connected)).await {
                            return End::ConsumerGone;
                        }
                        let join = self
                            .channels
                            .iter()
                            .map(|c| format!("#{c}"))
                            .collect::<Vec<_>>()
                            .join(",");
                        if let Err(e) = conn.send_text(format!("JOIN {join}")).await {
                            return lost(e, welcomed);
                        }
                    }
                    IrcInput::Ignore => {}
                }
            }
        }
    }
}
