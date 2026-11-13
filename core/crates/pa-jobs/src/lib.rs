// SPDX-License-Identifier: MPL-2.0
//! Persistent job queue of the core (CORE-011, docs/research/04 §1.10,
//! docs/dev/architecture.md §6).
//!
//! One queue serves the server and the clients: library scans, metadata
//! refresh, offline copies, transcodes, cleanup and, in full builds, Twitch
//! downloads and live recordings. Jobs live in the `jobs` table of the
//! library database (`pa-library`), so they survive a restart or a killed
//! process and resume from their checkpoint.
//!
//! - **States** — [`JobState`]: `queued → waiting_resources → running →
//!   paused / retrying → done / failed / cancelled`; every change is checked
//!   against [`JobState::TRANSITIONS`] and stored.
//! - **Priorities** — [`priority`]: live recording > what the user waits for
//!   > background. Jobs at [`priority::LIVE`] ignore concurrency limits.
//! - **Limits** — [`Limits`]: parallel runs per type (one scan, one
//!   transcode…).
//! - **Retries** — [`RunOutcome::Retryable`] retries with exponential
//!   [`Backoff`] until `max_attempts` runs failed.
//! - **Conditions** — [`NetworkPolicy`] (any network or Wi-Fi only) and
//!   battery: a job waits in `waiting_resources` or is paused while they are
//!   not met and continues when they are back ([`JobQueue::set_conditions`]).
//! - **Quotas** — a [`ResourceGate`] (free space, storage quotas: CORE-012)
//!   may hold a job before it starts or pause it later.
//! - **Events** — [`JobEvent`] through [`JobQueue::subscribe`]: added, state
//!   changed, progress, removed.
//! - **Restart** — [`JobQueue::new`] moves jobs a dead process left
//!   `running` back to `queued` with their checkpoint and `bytes_done`.
//!
//! [`JobStore`] is the synchronous core (SQL and every state change, explicit
//! timestamps); [`JobQueue`] adds handlers on worker threads, a background
//! scheduler ([`JobQueue::start`]) and the event channel. Handlers are plain
//! blocking code, like the rest of the I/O in the core (ADR-0013, ADR-0018).

mod model;
mod queue;
mod state;
mod store;

use pa_library::rusqlite;

pub use model::{
    Admission, Backoff, Conditions, Job, JobEvent, JobId, Limits, Network, NetworkPolicy, NewJob,
    PauseReason, ResourceGate, kinds, priority,
};
pub use queue::{Clock, JobContext, JobHandler, JobQueue, QueueConfig, QueueDriver, system_now_ms};
pub use serde_json;
pub use state::{JobState, UnknownState};
pub use store::{JobStore, RunOutcome, Schedule, ScheduleInput};

/// Error of the job queue.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("library: {0}")]
    Library(#[from] pa_library::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("job {0} not found")]
    NotFound(JobId),
    #[error("job {id}: transition {from} -> {to} is not allowed")]
    InvalidTransition {
        id: JobId,
        from: JobState,
        to: JobState,
    },
    #[error("job {0} was changed concurrently")]
    Conflict(JobId),
    #[error("job {0} is not finished")]
    NotFinished(JobId),
    #[error("invalid job type {0:?}: expected [a-z0-9_]+")]
    InvalidKind(String),
    #[error("invalid {0}")]
    Invalid(&'static str),
}
