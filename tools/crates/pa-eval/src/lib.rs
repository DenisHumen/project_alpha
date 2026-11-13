// SPDX-License-Identifier: MPL-2.0
//! `pa-eval` — offline eval runner for the recognition corpus (META-003).
//!
//! Pipeline: load and validate corpus JSONL ([`corpus`]) → run a
//! [`Recognizer`] on each case's input, with provider calls served from a
//! record/replay tape ([`provider`]) → score every labelled unit
//! ([`metrics`]) → JSON and Markdown report, regression comparison against a
//! stored baseline ([`report`]). Methodology: research 03 §6.2.
//! [`candidates`] measures candidate recall@k of the local title index
//! (META-007) without any provider calls; [`scorer`] adapts the scorer v0
//! (META-026) to [`Recognizer`].

pub mod candidates;
pub mod corpus;
pub mod metrics;
pub mod provider;
pub mod recognizer;
pub mod report;
pub mod scorer;

pub use corpus::{CaseInput, Corpus};
pub use metrics::{ItemOutcome, Metrics};
pub use provider::{Provider, ProviderRequest, RecordingProvider, ReplayProvider, Tape};
pub use recognizer::{FilePrediction, Recognizer, WorkGuess};
pub use report::{Comparison, DEFAULT_THRESHOLD, Report, compare, run};
