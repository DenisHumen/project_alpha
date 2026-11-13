// SPDX-License-Identifier: MPL-2.0
//! Test doubles (feature `testing`): a transport that replays recorded
//! responses and a clock whose sleeps only move time forward.
//!
//! Fixture format (one JSON file per response):
//!
//! ```json
//! { "status": 200, "headers": { "Ratelimit-Remaining": "799" }, "body": { "data": [] } }
//! ```

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::Deserialize;

use crate::transport::{BoxFuture, Clock, HttpRequest, HttpResponse, Transport, TransportError};

#[derive(Deserialize)]
struct FixtureFile {
    status: u16,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    body: serde_json::Value,
}

/// Parses a recorded response (format in the module docs).
///
/// # Panics
/// On malformed fixtures: they are test data.
pub fn fixture(json: &str) -> HttpResponse {
    let file: FixtureFile = serde_json::from_str(json).expect("malformed fixture");
    let body = if file.body.is_null() {
        Vec::new()
    } else {
        serde_json::to_vec(&file.body).expect("fixture body")
    };
    HttpResponse {
        status: file.status,
        headers: file.headers.into_iter().collect(),
        body,
    }
}

/// Replays queued responses in order and records every request.
#[derive(Debug, Default)]
pub struct FixtureTransport {
    queue: Mutex<VecDeque<Result<HttpResponse, TransportError>>>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl FixtureTransport {
    /// Empty transport; queue responses with [`FixtureTransport::push`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues a response.
    pub fn push(&self, response: HttpResponse) -> &Self {
        self.lock_queue().push_back(Ok(response));
        self
    }

    /// Queues a recorded fixture (see [`fixture`]).
    pub fn push_fixture(&self, json: &str) -> &Self {
        self.push(fixture(json))
    }

    /// Queues a network failure.
    pub fn push_error(&self, message: &str) -> &Self {
        self.lock_queue()
            .push_back(Err(TransportError(message.to_owned())));
        self
    }

    /// Requests sent so far.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Responses not consumed yet.
    pub fn remaining(&self) -> usize {
        self.lock_queue().len()
    }

    fn lock_queue(&self) -> std::sync::MutexGuard<'_, VecDeque<Result<HttpResponse, TransportError>>> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Transport for FixtureTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        let url = request.url.clone();
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(request);
        let next = self.lock_queue().pop_front().unwrap_or_else(|| {
            Err(TransportError(format!(
                "no fixture left for {url} (tests never use the network)"
            )))
        });
        Box::pin(async move { next })
    }
}

/// Clock that starts at a fixed time; `sleep` returns at once and advances
/// the time, and every sleep is recorded.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<SystemTime>,
    sleeps: Mutex<Vec<Duration>>,
}

impl ManualClock {
    /// Clock at `start`.
    pub fn new(start: SystemTime) -> Self {
        Self {
            now: Mutex::new(start),
            sleeps: Mutex::new(Vec::new()),
        }
    }

    /// Clock at `secs` seconds after the Unix epoch.
    pub fn at_unix(secs: u64) -> Self {
        Self::new(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    }

    /// Moves time forward without a sleep.
    pub fn advance(&self, by: Duration) {
        *self.now.lock().unwrap_or_else(|e| e.into_inner()) += by;
    }

    /// Sleeps requested so far.
    pub fn sleeps(&self) -> Vec<Duration> {
        self.sleeps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SystemTime {
        *self.now.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        self.sleeps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(duration);
        self.advance(duration);
        Box::pin(async {})
    }
}
