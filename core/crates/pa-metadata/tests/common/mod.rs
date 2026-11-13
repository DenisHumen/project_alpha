// SPDX-License-Identifier: MPL-2.0
//! Helpers shared by the provider integration tests.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pa_metadata::clock::{Clock, ManualClock};
use pa_metadata::http::{HttpRequest, HttpResponse, HttpTransport, TransportError};

/// Answers every request with a generated response and records the
/// (manual) time of each call: the load-test transport.
pub struct ClockedTransport {
    clock: Arc<ManualClock>,
    respond: Box<dyn Fn(&HttpRequest, Option<&str>) -> HttpResponse + Send + Sync>,
    pub calls: Mutex<Vec<(Duration, HttpRequest, Option<String>)>>,
}

impl ClockedTransport {
    pub fn new(
        clock: Arc<ManualClock>,
        respond: impl Fn(&HttpRequest, Option<&str>) -> HttpResponse + Send + Sync + 'static,
    ) -> Self {
        Self {
            clock,
            respond: Box::new(respond),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn times(&self) -> Vec<Duration> {
        self.calls.lock().unwrap().iter().map(|c| c.0).collect()
    }

    fn serve(&self, request: &HttpRequest, body: Option<&str>) -> HttpResponse {
        self.calls.lock().unwrap().push((
            self.clock.monotonic(),
            request.clone(),
            body.map(str::to_owned),
        ));
        (self.respond)(request, body)
    }
}

impl HttpTransport for ClockedTransport {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        Ok(self.serve(request, None))
    }

    fn post_json(&self, request: &HttpRequest, body: &str) -> Result<HttpResponse, TransportError> {
        Ok(self.serve(request, Some(body)))
    }
}

/// A transport without `POST` support (only the trait's default).
pub struct GetOnly;

impl HttpTransport for GetOnly {
    fn get(&self, _: &HttpRequest) -> Result<HttpResponse, TransportError> {
        Err(TransportError("offline".into()))
    }
}

/// Largest number of calls inside any half-open window `[t, t + window)`.
pub fn max_in_window(times: &[Duration], window: Duration) -> usize {
    let mut sorted = times.to_vec();
    sorted.sort();
    let mut best = 0;
    let mut end = 0;
    for start in 0..sorted.len() {
        while end < sorted.len() && sorted[end] < sorted[start] + window {
            end += 1;
        }
        best = best.max(end - start);
    }
    best
}

pub fn ok(body: &str, headers: &[(&str, &str)]) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        body: body.to_owned(),
    }
}
