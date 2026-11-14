// SPDX-License-Identifier: MPL-2.0
//! Test doubles (feature `testing`): a WebSocket connector that replays
//! scripted frames per connection and records what the client sent. Tests
//! never touch the network.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use pa_twitch::BoxFuture;

use crate::source::ChatError;
use crate::ws::{WsConnection, WsConnector};

/// What a scripted connection does after its last frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptEnd {
    /// The peer closes the socket.
    Close,
    /// The socket fails with this error.
    Error(String),
    /// Nothing more ever arrives (exercises idle/keepalive timeouts).
    Hang,
}

/// Frames one connection delivers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    /// Text frames in order.
    pub frames: Vec<String>,
    /// Behaviour after the frames.
    pub end: ScriptEnd,
}

impl Script {
    /// Frames, then the peer closes.
    pub fn closing(frames: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            frames: frames.into_iter().map(Into::into).collect(),
            end: ScriptEnd::Close,
        }
    }

    /// Frames, then silence.
    pub fn hanging(frames: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            frames: frames.into_iter().map(Into::into).collect(),
            end: ScriptEnd::Hang,
        }
    }
}

#[derive(Debug, Default)]
struct Shared {
    scripts: VecDeque<Result<Script, String>>,
    urls: Vec<String>,
    sent: Vec<(usize, String)>,
}

/// Connector that serves queued [`Script`]s, one per `connect`. When the
/// queue is empty, `connect` fails.
#[derive(Debug, Default, Clone)]
pub struct ScriptedConnector {
    shared: Arc<Mutex<Shared>>,
}

impl ScriptedConnector {
    /// Empty connector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues a connection.
    pub fn push(&self, script: Script) -> &Self {
        self.lock().scripts.push_back(Ok(script));
        self
    }

    /// Queues a failed connection attempt.
    pub fn push_connect_error(&self, message: &str) -> &Self {
        self.lock().scripts.push_back(Err(message.to_owned()));
        self
    }

    /// URLs connected to, in order.
    pub fn urls(&self) -> Vec<String> {
        self.lock().urls.clone()
    }

    /// Frames sent by the client as `(connection index, text)`.
    pub fn sent(&self) -> Vec<(usize, String)> {
        self.lock().sent.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl WsConnector for ScriptedConnector {
    fn connect<'a>(
        &'a self,
        url: &'a str,
    ) -> BoxFuture<'a, Result<Box<dyn WsConnection>, ChatError>> {
        let mut shared = self.lock();
        let index = shared.urls.len();
        shared.urls.push(url.to_owned());
        let next = shared.scripts.pop_front();
        drop(shared);
        let result = match next {
            Some(Ok(script)) => Ok(Box::new(ScriptedConnection {
                index,
                frames: script.frames.into(),
                end: script.end,
                shared: self.shared.clone(),
            }) as Box<dyn WsConnection>),
            Some(Err(message)) => Err(ChatError::Connect(message)),
            None => Err(ChatError::Connect(
                "no scripted connection left (tests never use the network)".to_owned(),
            )),
        };
        Box::pin(async move { result })
    }
}

struct ScriptedConnection {
    index: usize,
    frames: VecDeque<String>,
    end: ScriptEnd,
    shared: Arc<Mutex<Shared>>,
}

impl WsConnection for ScriptedConnection {
    fn send_text(&mut self, text: String) -> BoxFuture<'_, Result<(), ChatError>> {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sent
            .push((self.index, text));
        Box::pin(async { Ok(()) })
    }

    fn recv_text(&mut self) -> BoxFuture<'_, Option<Result<String, ChatError>>> {
        if let Some(frame) = self.frames.pop_front() {
            return Box::pin(async move { Some(Ok(frame)) });
        }
        match self.end.clone() {
            ScriptEnd::Close => Box::pin(async { None }),
            ScriptEnd::Error(e) => {
                // Report the error once, then behave as closed.
                self.end = ScriptEnd::Close;
                Box::pin(async move { Some(Err(ChatError::Socket(e))) })
            }
            ScriptEnd::Hang => Box::pin(std::future::pending()),
        }
    }

    fn close(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}
