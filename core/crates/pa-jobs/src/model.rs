// SPDX-License-Identifier: MPL-2.0
//! Jobs, their requirements, scheduling policy and events.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde_json::Value;

use crate::JobState;

/// Row id of a job in the `jobs` table.
pub type JobId = i64;

/// Job types known to the core. A type is any `[a-z0-9_]+` string: other
/// crates register their own (the Twitch types live in `pa-twitch` and exist
/// only in full builds, ADR-0007).
pub mod kinds {
    /// Scan a library source (`pa-scanner`).
    pub const LIBRARY_SCAN: &str = "library_scan";
    /// Refresh metadata of works (`pa-metadata`).
    pub const METADATA_REFRESH: &str = "metadata_refresh";
    /// Copy a library file to the device, optionally transcoded.
    pub const OFFLINE_COPY: &str = "offline_copy";
    /// Transcode a file into an optimized version.
    pub const TRANSCODE_OPTIMIZE: &str = "transcode_optimize";
    /// Retention, trash and tmp cleanup (`pa-storage`, CORE-012).
    pub const CLEANUP: &str = "cleanup";

    /// True if `kind` is a valid job type: non-empty `[a-z0-9_]+` (the same
    /// check as the `jobs.type` column).
    pub fn is_valid(kind: &str) -> bool {
        !kind.is_empty()
            && kind
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    }
}

/// Priorities. Higher runs first; jobs of equal priority run in creation
/// order.
pub mod priority {
    /// Background work: scans, metadata, cleanup.
    pub const BACKGROUND: i64 = 0;
    /// Something the user waits for ("watch offline", a manual rescan).
    pub const USER_WAITING: i64 = 100;
    /// Live recording. Jobs at or above this priority ignore the
    /// concurrency limit of their type: a stream that is not recorded now is
    /// lost.
    pub const LIVE: i64 = 1000;
}

/// What a job needs from the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NetworkPolicy {
    /// Local work (scan of a local folder, cleanup).
    #[default]
    None,
    /// Any connection.
    Any,
    /// Wi-Fi or Ethernet only, not a metered (mobile) connection.
    Unmetered,
}

impl NetworkPolicy {
    /// Name stored in `jobs.network`.
    pub const fn as_str(self) -> &'static str {
        match self {
            NetworkPolicy::None => "none",
            NetworkPolicy::Any => "any",
            NetworkPolicy::Unmetered => "unmetered",
        }
    }

    fn allows(self, network: Network) -> bool {
        match self {
            NetworkPolicy::None => true,
            NetworkPolicy::Any => network != Network::Offline,
            NetworkPolicy::Unmetered => network == Network::Unmetered,
        }
    }
}

impl FromStr for NetworkPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(NetworkPolicy::None),
            "any" => Ok(NetworkPolicy::Any),
            "unmetered" => Ok(NetworkPolicy::Unmetered),
            _ => Err(format!("unknown network policy {s:?}")),
        }
    }
}

/// Current network of the device, reported by the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Network {
    Offline,
    /// Mobile data or a hotspot marked as metered.
    Metered,
    /// Wi-Fi or Ethernet.
    #[default]
    Unmetered,
}

/// Device conditions the scheduler checks before starting a job and while
/// it runs. Defaults fit a desktop or a server: online, on mains power.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Conditions {
    pub network: Network,
    pub on_battery: bool,
}

/// Why a job is paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PauseReason {
    /// Paused by the user; stays paused until [`resume`](crate::JobQueue::resume).
    User,
    /// The network it needs went away; resumed automatically.
    Network,
    /// The device switched to battery; resumed automatically.
    Battery,
    /// The resource gate (free space, CORE-012) stopped it; resumed when the
    /// gate admits it again.
    Resources,
}

impl PauseReason {
    /// Name stored in `jobs.pause_reason`.
    pub const fn as_str(self) -> &'static str {
        match self {
            PauseReason::User => "user",
            PauseReason::Network => "network",
            PauseReason::Battery => "battery",
            PauseReason::Resources => "resources",
        }
    }
}

impl FromStr for PauseReason {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(PauseReason::User),
            "network" => Ok(PauseReason::Network),
            "battery" => Ok(PauseReason::Battery),
            "resources" => Ok(PauseReason::Resources),
            _ => Err(format!("unknown pause reason {s:?}")),
        }
    }
}

impl fmt::Display for PauseReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A job to put into the queue.
#[derive(Debug, Clone, PartialEq)]
pub struct NewJob {
    /// Job type, `[a-z0-9_]+` (see [`kinds`]).
    pub kind: String,
    /// Handler input. Must be a JSON object.
    pub params: Value,
    pub priority: i64,
    pub network: NetworkPolicy,
    pub allow_on_battery: bool,
    /// Runs before a retryable error becomes `failed`; at least 1.
    pub max_attempts: u32,
    /// Do not start before this time (scheduled cleanup).
    pub not_before_ms: Option<i64>,
    pub owner_profile_id: Option<i64>,
    /// Storage target id (CORE-012).
    pub target_storage: Option<String>,
    pub bytes_total: Option<u64>,
    pub created_by_device: Option<String>,
}

impl NewJob {
    /// A background job of `kind` with `params`, no network, any power,
    /// five attempts.
    pub fn new(kind: impl Into<String>, params: Value) -> Self {
        Self {
            kind: kind.into(),
            params,
            priority: priority::BACKGROUND,
            network: NetworkPolicy::None,
            allow_on_battery: true,
            max_attempts: 5,
            not_before_ms: None,
            owner_profile_id: None,
            target_storage: None,
            bytes_total: None,
            created_by_device: None,
        }
    }

    pub fn priority(mut self, priority: i64) -> Self {
        self.priority = priority;
        self
    }

    pub fn network(mut self, network: NetworkPolicy) -> Self {
        self.network = network;
        self
    }

    pub fn allow_on_battery(mut self, allow: bool) -> Self {
        self.allow_on_battery = allow;
        self
    }

    pub fn max_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts;
        self
    }

    pub fn not_before_ms(mut self, at_ms: i64) -> Self {
        self.not_before_ms = Some(at_ms);
        self
    }

    pub fn owner(mut self, profile_id: i64) -> Self {
        self.owner_profile_id = Some(profile_id);
        self
    }

    pub fn target_storage(mut self, target: impl Into<String>) -> Self {
        self.target_storage = Some(target.into());
        self
    }

    pub fn bytes_total(mut self, total: u64) -> Self {
        self.bytes_total = Some(total);
        self
    }

    pub fn created_by_device(mut self, device: impl Into<String>) -> Self {
        self.created_by_device = Some(device.into());
        self
    }
}

/// A row of the `jobs` table.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: JobId,
    pub kind: String,
    pub state: JobState,
    pub priority: i64,
    pub params: Value,
    /// Handler state saved for resuming (downloaded segments, offsets).
    pub checkpoint: Option<Value>,
    pub result: Option<Value>,
    /// Last error message (kept after a successful retry for diagnostics).
    pub error: Option<String>,
    /// 0.0..=1.0, if known.
    pub progress: Option<f64>,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    /// Runs started so far.
    pub attempts: u32,
    pub max_attempts: u32,
    /// Retry or scheduled start time.
    pub next_run_at_ms: Option<i64>,
    pub network: NetworkPolicy,
    pub allow_on_battery: bool,
    pub pause_reason: Option<PauseReason>,
    pub wait_reason: Option<String>,
    pub owner_profile_id: Option<i64>,
    pub target_storage: Option<String>,
    pub created_by_device: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl Job {
    /// The condition of `conditions` that keeps this job from running, if
    /// any.
    pub fn blocked_by(&self, conditions: &Conditions) -> Option<PauseReason> {
        if !self.network.allows(conditions.network) {
            Some(PauseReason::Network)
        } else if conditions.on_battery && !self.allow_on_battery {
            Some(PauseReason::Battery)
        } else {
            None
        }
    }
}

/// Answer of a [`ResourceGate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Admit,
    /// Do not start now; the reason goes to `wait_reason`.
    Wait(String),
}

/// Extra admission check before a job starts: free space and quotas of the
/// storage target (`pa-storage`, CORE-012). Called with the queue lock held;
/// must be fast and must not call back into the queue.
pub trait ResourceGate: Send + Sync {
    fn admit(&self, job: &Job) -> Admission;
}

impl<F> ResourceGate for F
where
    F: Fn(&Job) -> Admission + Send + Sync,
{
    fn admit(&self, job: &Job) -> Admission {
        self(job)
    }
}

/// Concurrency limits per job type. A job with priority at or above
/// [`priority::LIVE`] is not limited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    per_kind: HashMap<String, usize>,
    default: usize,
}

impl Default for Limits {
    /// 04 §1.10: one scan, one transcode, one cleanup at a time; two
    /// metadata refreshes and two copies; two of any other type.
    fn default() -> Self {
        let per_kind = [
            (kinds::LIBRARY_SCAN, 1),
            (kinds::METADATA_REFRESH, 2),
            (kinds::OFFLINE_COPY, 2),
            (kinds::TRANSCODE_OPTIMIZE, 1),
            (kinds::CLEANUP, 1),
        ]
        .into_iter()
        .map(|(k, n)| (k.to_owned(), n))
        .collect();
        Self {
            per_kind,
            default: 2,
        }
    }
}

impl Limits {
    /// Limit for `kind`. Zero means "do not start" (the type is disabled).
    pub fn get(&self, kind: &str) -> usize {
        self.per_kind.get(kind).copied().unwrap_or(self.default)
    }

    pub fn set(&mut self, kind: impl Into<String>, limit: usize) {
        self.per_kind.insert(kind.into(), limit);
    }

    /// Limit of types without their own.
    pub fn set_default(&mut self, limit: usize) {
        self.default = limit;
    }
}

/// Exponential backoff between retries: `base · 2^(attempt-1)`, at most
/// `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub base: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(5),
            max: Duration::from_secs(3600),
        }
    }
}

impl Backoff {
    /// Delay after the `attempt`-th failed run (1-based).
    pub fn delay(&self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(31);
        self.base
            .checked_mul(1u32 << shift)
            .map_or(self.max, |d| d.min(self.max))
    }
}

/// Event of the queue, for the UI and for WS clients (`job.progress`,
/// `job.done`).
#[derive(Debug, Clone, PartialEq)]
pub enum JobEvent {
    Added {
        id: JobId,
        kind: String,
    },
    StateChanged {
        id: JobId,
        kind: String,
        from: JobState,
        to: JobState,
    },
    Progress {
        id: JobId,
        progress: Option<f64>,
        bytes_done: u64,
        bytes_total: Option<u64>,
    },
    Removed {
        id: JobId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let b = Backoff {
            base: Duration::from_secs(5),
            max: Duration::from_secs(60),
        };
        assert_eq!(b.delay(1), Duration::from_secs(5));
        assert_eq!(b.delay(2), Duration::from_secs(10));
        assert_eq!(b.delay(4), Duration::from_secs(40));
        assert_eq!(b.delay(5), Duration::from_secs(60));
        assert_eq!(b.delay(u32::MAX), Duration::from_secs(60));
    }

    #[test]
    fn kind_names() {
        assert!(kinds::is_valid(kinds::LIBRARY_SCAN));
        assert!(!kinds::is_valid(""));
        assert!(!kinds::is_valid("Scan"));
        assert!(!kinds::is_valid("a-b"));
    }

    #[test]
    fn requirements() {
        let mut job_conditions = Conditions::default();
        let policy = NetworkPolicy::Unmetered;
        assert!(policy.allows(job_conditions.network));
        job_conditions.network = Network::Metered;
        assert!(!policy.allows(job_conditions.network));
        assert!(NetworkPolicy::Any.allows(Network::Metered));
        assert!(!NetworkPolicy::Any.allows(Network::Offline));
        assert!(NetworkPolicy::None.allows(Network::Offline));
    }
}
