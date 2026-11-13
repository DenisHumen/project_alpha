// SPDX-License-Identifier: MPL-2.0
//! Provider access with record/replay.
//!
//! The recognition core is sans-IO (research 03 §7.2): it talks to metadata
//! providers through a trait. The eval runner plugs in a [`ReplayProvider`]
//! that answers from a *tape* of previously recorded responses, so a run is
//! offline and deterministic. A [`RecordingProvider`] wraps any live
//! provider (`pa_metadata::tmdb::TmdbClient` and later ones) and writes such
//! a tape. Request, error and trait types are the core's own
//! ([`pa_recognition::provider`]), re-exported here, so the same recognizer
//! code runs against live providers and tapes.
//!
//! Tape format: JSONL, one `{"request": {...}, "response": ...}` per line,
//! sorted by the canonical JSON of the request, without duplicates.

use std::collections::BTreeMap;

pub use pa_recognition::provider::{Provider, ProviderError, ProviderRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Recorded request/response pairs.
#[derive(Debug, Clone, Default)]
pub struct Tape {
    entries: BTreeMap<String, (ProviderRequest, Value)>,
}

#[derive(Serialize, Deserialize)]
struct TapeLine {
    request: ProviderRequest,
    response: Value,
}

impl Tape {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut tape = Tape::default();
        for (idx, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: TapeLine =
                serde_json::from_str(line).map_err(|e| format!("tape line {}: {e}", idx + 1))?;
            tape.insert(entry.request, entry.response);
        }
        Ok(tape)
    }

    pub fn insert(&mut self, request: ProviderRequest, response: Value) {
        self.entries.insert(request.key(), (request, response));
    }

    pub fn get(&self, request: &ProviderRequest) -> Option<&Value> {
        self.entries.get(&request.key()).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Serializes in canonical (sorted) order, so re-recording the same
    /// responses gives a byte-identical tape.
    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        for (request, response) in self.entries.values() {
            let line = TapeLine {
                request: request.clone(),
                response: response.clone(),
            };
            out.push_str(&serde_json::to_string(&line).expect("tape line serializes"));
            out.push('\n');
        }
        out
    }
}

/// Answers from a tape; unknown requests are [`ProviderError::TapeMiss`].
#[derive(Debug, Default)]
pub struct ReplayProvider {
    tape: Tape,
}

impl ReplayProvider {
    pub fn new(tape: Tape) -> Self {
        Self { tape }
    }
}

impl Provider for ReplayProvider {
    fn call(&mut self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        self.tape
            .get(request)
            .cloned()
            .ok_or_else(|| ProviderError::TapeMiss(request.key()))
    }
}

/// Passes calls to `inner` and records successful responses.
pub struct RecordingProvider<P> {
    inner: P,
    tape: Tape,
}

impl<P: Provider> RecordingProvider<P> {
    pub fn new(inner: P) -> Self {
        Self {
            inner,
            tape: Tape::default(),
        }
    }

    pub fn into_tape(self) -> Tape {
        self.tape
    }
}

impl<P: Provider> Provider for RecordingProvider<P> {
    fn call(&mut self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        let response = self.inner.call(request)?;
        self.tape.insert(request.clone(), response.clone());
        Ok(response)
    }
}

/// Counts calls and tape misses for the performance section of the report.
pub(crate) struct Counting<'a> {
    pub inner: &'a mut dyn Provider,
    pub calls: u64,
    pub misses: u64,
}

impl Provider for Counting<'_> {
    fn call(&mut self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        self.calls += 1;
        let result = self.inner.call(request);
        if matches!(result, Err(ProviderError::TapeMiss(_))) {
            self.misses += 1;
        }
        result
    }
}
