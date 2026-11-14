// SPDX-License-Identifier: MPL-2.0
//! Signed-in chat: EventSub WebSocket for reading, Helix
//! `POST /chat/messages` for sending (research 06 §3.1, the recommended
//! Twitch path). Needs `user:read:chat`; sending needs `user:write:chat`.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use pa_twitch::{
    BoxFuture, Clock, EventSubSubscriptionRequest, MAX_CHAT_MESSAGE_CHARS, SendChatMessage,
    SentChatMessage,
};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use crate::eventsub::{CHAT_SUBSCRIPTIONS, EventSubFrame, SUB_CHAT_MESSAGE, parse_frame};
use crate::model::{ChatEvent, SourceStatus};
use crate::source::{Backoff, ChatApi, ChatError, ChatSource, SourceKind};
use crate::ws::{WsConnection, WsConnector};

/// EventSub WebSocket endpoint.
pub const DEFAULT_EVENTSUB_URL: &str = "wss://eventsub.wss.twitch.tv/ws";

/// Frame ids remembered for de-duplication.
const DEDUP_WINDOW: usize = 1024;

/// Configuration of [`EventSubChatSource`].
#[derive(Debug, Clone)]
pub struct EventSubConfig {
    /// WebSocket URL.
    pub url: String,
    /// The signed-in user (`user_id` of the subscription condition and the
    /// `sender_id` of sent messages).
    pub user_id: String,
    /// Channels (broadcaster ids) to read.
    pub broadcaster_ids: Vec<String>,
    /// Reconnect backoff.
    pub backoff: Backoff,
    /// Added to the session's keepalive timeout before the connection is
    /// considered dead.
    pub keepalive_grace: Duration,
    /// Keepalive timeout when the welcome does not state one.
    pub default_keepalive: Duration,
    /// How long to wait for `session_welcome` after connecting.
    pub welcome_timeout: Duration,
    /// Give up after this many failed attempts in a row (`None`: never).
    pub max_attempts: Option<u32>,
    /// Events buffered for a slow consumer.
    pub buffer: usize,
}

impl EventSubConfig {
    /// Defaults for a user reading the given channels.
    pub fn new(
        user_id: impl Into<String>,
        broadcaster_ids: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            url: DEFAULT_EVENTSUB_URL.to_owned(),
            user_id: user_id.into(),
            broadcaster_ids: broadcaster_ids.into_iter().map(Into::into).collect(),
            backoff: Backoff::default(),
            keepalive_grace: Duration::from_secs(5),
            default_keepalive: Duration::from_secs(10),
            welcome_timeout: Duration::from_secs(10),
            max_attempts: None,
            buffer: 4096,
        }
    }
}

/// Signed-in chat source. Must be created inside a Tokio runtime.
pub struct EventSubChatSource {
    api: Arc<dyn ChatApi>,
    user_id: String,
    events: Mutex<mpsc::Receiver<ChatEvent>>,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for EventSubChatSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventSubChatSource")
            .field("user_id", &self.user_id)
            .finish_non_exhaustive()
    }
}

impl EventSubChatSource {
    /// Starts the connection loop.
    pub fn spawn(
        config: EventSubConfig,
        api: Arc<dyn ChatApi>,
        connector: Arc<dyn WsConnector>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, ChatError> {
        if config.user_id.is_empty() {
            return Err(ChatError::InvalidArgument("user id is required"));
        }
        if config.broadcaster_ids.is_empty() {
            return Err(ChatError::InvalidArgument(
                "at least one channel is required",
            ));
        }
        let (tx, rx) = mpsc::channel(config.buffer.max(1));
        let user_id = config.user_id.clone();
        let driver = Driver {
            config,
            api: api.clone(),
            connector,
            clock,
            tx,
            seen: VecDeque::new(),
            seen_set: HashSet::new(),
        };
        Ok(Self {
            api,
            user_id,
            events: Mutex::new(rx),
            task: tokio::spawn(driver.run()),
        })
    }

    /// Starts with the real `wss://` transport and system clock.
    #[cfg(feature = "ws")]
    pub fn connect(config: EventSubConfig, api: Arc<dyn ChatApi>) -> Result<Self, ChatError> {
        Self::spawn(
            config,
            api,
            Arc::new(crate::ws::TungsteniteConnector::new()?),
            Arc::new(pa_twitch::SystemClock),
        )
    }
}

impl Drop for EventSubChatSource {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ChatSource for EventSubChatSource {
    fn kind(&self) -> SourceKind {
        SourceKind::EventSub
    }

    fn can_send(&self) -> bool {
        true
    }

    fn next_event(&self) -> BoxFuture<'_, Option<ChatEvent>> {
        Box::pin(async move { self.events.lock().await.recv().await })
    }

    fn send_message<'a>(
        &'a self,
        broadcaster_id: &'a str,
        text: &'a str,
        reply_to: Option<&'a str>,
    ) -> BoxFuture<'a, Result<SentChatMessage, ChatError>> {
        Box::pin(async move {
            let text = text.trim();
            let chars = text.chars().count();
            if chars == 0 || chars > MAX_CHAT_MESSAGE_CHARS {
                return Err(ChatError::InvalidArgument(
                    "chat message must be 1-500 characters",
                ));
            }
            self.api
                .send(SendChatMessage {
                    broadcaster_id: broadcaster_id.to_owned(),
                    sender_id: self.user_id.clone(),
                    message: text.to_owned(),
                    reply_parent_message_id: reply_to.map(str::to_owned),
                })
                .await
        })
    }

    fn close(&self) {
        self.task.abort();
    }
}

enum End {
    ConsumerGone,
    /// Auth gone or subscriptions revoked: do not retry.
    Fatal(String),
    Lost {
        reason: String,
        healthy: bool,
    },
}

struct Driver {
    config: EventSubConfig,
    api: Arc<dyn ChatApi>,
    connector: Arc<dyn WsConnector>,
    clock: Arc<dyn Clock>,
    tx: mpsc::Sender<ChatEvent>,
    seen: VecDeque<String>,
    seen_set: HashSet<String>,
}

enum Wait {
    Frame(EventSubFrame),
    Closed(String),
    Timeout,
}

impl Driver {
    async fn emit(&self, event: ChatEvent) -> bool {
        self.tx.send(event).await.is_ok()
    }

    async fn run(mut self) {
        let mut attempt: u32 = 0;
        loop {
            let url = self.config.url.clone();
            let end = match self.connector.connect(&url).await {
                Ok(conn) => self.session(conn).await,
                Err(e) => End::Lost {
                    reason: e.to_string(),
                    healthy: false,
                },
            };
            let (reason, healthy) = match end {
                End::ConsumerGone => return,
                End::Fatal(reason) => {
                    let _ = self
                        .emit(ChatEvent::Status(SourceStatus::Stopped { reason }))
                        .await;
                    return;
                }
                End::Lost { reason, healthy } => (reason, healthy),
            };
            attempt = if healthy { 1 } else { attempt + 1 };
            if self.config.max_attempts.is_some_and(|max| attempt > max) {
                let _ = self
                    .emit(ChatEvent::Status(SourceStatus::Stopped { reason }))
                    .await;
                return;
            }
            let delay = self.config.backoff.delay(attempt);
            let status = SourceStatus::Reconnecting {
                attempt,
                delay,
                reason,
            };
            if !self.emit(ChatEvent::Status(status)).await {
                return;
            }
            self.clock.sleep(delay).await;
        }
    }

    /// Next decoded frame, or why there is none.
    async fn wait(&self, conn: &mut dyn WsConnection, timeout: Duration) -> Wait {
        let frame = tokio::select! {
            biased;
            frame = conn.recv_text() => frame,
            () = self.clock.sleep(timeout) => return Wait::Timeout,
        };
        match frame {
            None => Wait::Closed("connection closed by Twitch".to_owned()),
            Some(Err(e)) => Wait::Closed(e.to_string()),
            Some(Ok(text)) => match parse_frame(&text) {
                Ok(frame) => Wait::Frame(frame),
                // One undecodable frame is not worth a reconnect.
                Err(e) => Wait::Frame(EventSubFrame::Unknown(e.to_string())),
            },
        }
    }

    /// Waits for `session_welcome`; returns the session id and keepalive.
    async fn welcome(&self, conn: &mut dyn WsConnection) -> Result<(String, Duration), End> {
        loop {
            match self.wait(conn, self.config.welcome_timeout).await {
                Wait::Frame(EventSubFrame::Welcome {
                    session_id,
                    keepalive_timeout,
                }) => {
                    return Ok((
                        session_id,
                        keepalive_timeout.unwrap_or(self.config.default_keepalive),
                    ));
                }
                Wait::Frame(_) => continue,
                Wait::Closed(reason) => {
                    return Err(End::Lost {
                        reason,
                        healthy: false,
                    });
                }
                Wait::Timeout => {
                    return Err(End::Lost {
                        reason: "no session_welcome from Twitch".to_owned(),
                        healthy: false,
                    });
                }
            }
        }
    }

    async fn subscribe_all(&self, session_id: &str) -> Result<(), End> {
        for broadcaster in &self.config.broadcaster_ids {
            for kind in CHAT_SUBSCRIPTIONS {
                let request = EventSubSubscriptionRequest {
                    kind: (*kind).to_owned(),
                    version: "1".to_owned(),
                    condition: BTreeMap::from([
                        ("broadcaster_user_id".to_owned(), broadcaster.clone()),
                        ("user_id".to_owned(), self.config.user_id.clone()),
                    ]),
                    session_id: session_id.to_owned(),
                };
                match self.api.subscribe(request).await {
                    Ok(_) => {}
                    Err(ChatError::Unauthorized) => {
                        return Err(End::Fatal("Twitch sign-in is no longer valid".to_owned()));
                    }
                    // Without chat messages the session is useless; retry.
                    Err(e) if *kind == SUB_CHAT_MESSAGE => {
                        return Err(End::Lost {
                            reason: e.to_string(),
                            healthy: false,
                        });
                    }
                    // Moderation feeds are best effort (e.g. a channel that
                    // restricts them): tell the UI and carry on.
                    Err(e) => {
                        let notice = ChatEvent::Notice {
                            channel_login: None,
                            notice_id: Some("subscription_failed".to_owned()),
                            text: format!("{kind} for {broadcaster}: {e}"),
                        };
                        if !self.emit(notice).await {
                            return Err(End::ConsumerGone);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn first_time(&mut self, message_id: &str) -> bool {
        if message_id.is_empty() {
            return true;
        }
        if !self.seen_set.insert(message_id.to_owned()) {
            return false;
        }
        self.seen.push_back(message_id.to_owned());
        if self.seen.len() > DEDUP_WINDOW
            && let Some(old) = self.seen.pop_front()
        {
            self.seen_set.remove(&old);
        }
        true
    }

    async fn session(&mut self, conn: Box<dyn WsConnection>) -> End {
        let mut conn = conn;
        let (session_id, mut keepalive) = match self.welcome(conn.as_mut()).await {
            Ok(w) => w,
            Err(end) => {
                conn.close().await;
                return end;
            }
        };
        if let Err(end) = self.subscribe_all(&session_id).await {
            conn.close().await;
            return end;
        }
        if !self.emit(ChatEvent::Status(SourceStatus::Connected)).await {
            return End::ConsumerGone;
        }
        loop {
            let timeout = keepalive + self.config.keepalive_grace;
            let frame = match self.wait(conn.as_mut(), timeout).await {
                Wait::Frame(f) => f,
                Wait::Closed(reason) => {
                    conn.close().await;
                    return End::Lost {
                        reason,
                        healthy: true,
                    };
                }
                Wait::Timeout => {
                    conn.close().await;
                    return End::Lost {
                        reason: "keepalive timeout".to_owned(),
                        healthy: true,
                    };
                }
            };
            match frame {
                EventSubFrame::Notification {
                    message_id, event, ..
                } => {
                    if let Some(event) = event.filter(|_| self.first_time(&message_id))
                        && !self.emit(event).await
                    {
                        return End::ConsumerGone;
                    }
                }
                EventSubFrame::Reconnect { reconnect_url } => {
                    // Subscriptions move with the session: connect to the
                    // new URL, wait for its welcome, then drop the old one.
                    let moved = match self.connector.connect(&reconnect_url).await {
                        Ok(mut next) => match self.welcome(next.as_mut()).await {
                            Ok((_, k)) => Ok((next, k)),
                            Err(end) => Err(end),
                        },
                        Err(e) => Err(End::Lost {
                            reason: e.to_string(),
                            healthy: true,
                        }),
                    };
                    conn.close().await;
                    match moved {
                        Ok((next, k)) => {
                            conn = next;
                            keepalive = k;
                        }
                        Err(End::Lost { reason, .. }) => {
                            return End::Lost {
                                reason,
                                healthy: true,
                            };
                        }
                        Err(end) => return end,
                    }
                }
                EventSubFrame::Revocation {
                    subscription_type,
                    status,
                } => {
                    if status == "authorization_revoked" || subscription_type == SUB_CHAT_MESSAGE {
                        conn.close().await;
                        return End::Fatal(format!("Twitch revoked {subscription_type}: {status}"));
                    }
                    let notice = ChatEvent::Notice {
                        channel_login: None,
                        notice_id: Some("subscription_revoked".to_owned()),
                        text: format!("{subscription_type}: {status}"),
                    };
                    if !self.emit(notice).await {
                        return End::ConsumerGone;
                    }
                }
                EventSubFrame::Welcome { .. }
                | EventSubFrame::Keepalive
                | EventSubFrame::Unknown(_) => {}
            }
        }
    }
}
