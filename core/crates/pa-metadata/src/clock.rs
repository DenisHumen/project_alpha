// SPDX-License-Identifier: MPL-2.0
//! Time source for rate limiting, backoff and cache expiry.
//!
//! Injected so tests never sleep for real: [`ManualClock`] records requested
//! sleeps and advances its own time instead.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Monotonic time, wall-clock time and sleeping.
pub trait Clock: Send + Sync {
    /// Monotonic time since an arbitrary origin.
    fn monotonic(&self) -> Duration;
    /// Seconds since the Unix epoch (cache timestamps).
    fn unix_secs(&self) -> u64;
    /// Blocks the current thread.
    fn sleep(&self, duration: Duration);
}

/// The real clock.
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn monotonic(&self) -> Duration {
        self.origin.elapsed()
    }

    fn unix_secs(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// A clock that only moves when told to; `sleep` advances it and is
/// recorded. For tests and deterministic replays.
#[derive(Debug, Default)]
pub struct ManualClock {
    state: Mutex<ManualState>,
}

#[derive(Debug, Default)]
struct ManualState {
    elapsed: Duration,
    unix_origin: u64,
    sleeps: Vec<Duration>,
}

impl ManualClock {
    /// A clock whose wall time starts at `unix_secs`.
    pub fn at_unix(unix_secs: u64) -> Self {
        Self {
            state: Mutex::new(ManualState {
                unix_origin: unix_secs,
                ..ManualState::default()
            }),
        }
    }

    /// Moves time forward without recording a sleep.
    pub fn advance(&self, by: Duration) {
        self.lock().elapsed += by;
    }

    /// Sleeps requested so far, in order.
    pub fn sleeps(&self) -> Vec<Duration> {
        self.lock().sleeps.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ManualState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Clock for ManualClock {
    fn monotonic(&self) -> Duration {
        self.lock().elapsed
    }

    fn unix_secs(&self) -> u64 {
        let s = self.lock();
        s.unix_origin + s.elapsed.as_secs()
    }

    fn sleep(&self, duration: Duration) {
        let mut s = self.lock();
        s.elapsed += duration;
        s.sleeps.push(duration);
    }
}
