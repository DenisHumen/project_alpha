// SPDX-License-Identifier: MPL-2.0
//! Hybrid logical clock (Kulkarni et al., 2014).
//!
//! A timestamp is `(wall_ms, counter, device)`. It stays close to physical
//! time, never goes backwards on one device and is unique across devices
//! because the device id is part of it, so timestamps give every operation a
//! total order without a server.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::Error;

/// Remote timestamps further ahead of the local physical clock than this are
/// rejected: a peer with a broken clock must not drag every device forward.
pub const MAX_DRIFT_MS: u64 = 5 * 60 * 1000;

/// Maximum length of a device id.
pub const MAX_DEVICE_ID_LEN: usize = 64;

/// One HLC timestamp. The derived order (wall, counter, device) is the
/// last-writer-wins order of pa-sync.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Hlc {
    /// Unix milliseconds (physical part).
    pub wall_ms: u64,
    /// Logical part: orders events within one millisecond.
    pub counter: u32,
    /// Device that issued the timestamp; breaks ties between devices.
    pub device: String,
}

impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}@{}", self.wall_ms, self.counter, self.device)
    }
}

/// True for a usable device id: 1..=64 ASCII letters, digits, `-` or `_`.
pub fn is_valid_device_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_DEVICE_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Current Unix time in milliseconds (0 if the clock is before 1970).
pub fn system_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The clock of one device. Physical time is passed in, so tests are
/// deterministic; [`HlcClock::now`] reads the system clock.
#[derive(Debug, Clone)]
pub struct HlcClock {
    device: String,
    wall_ms: u64,
    counter: u32,
}

impl HlcClock {
    /// A clock for `device` (see [`is_valid_device_id`]).
    pub fn new(device: impl Into<String>) -> Result<Self, Error> {
        let device = device.into();
        if !is_valid_device_id(&device) {
            return Err(Error::Invalid("device id"));
        }
        Ok(Self {
            device,
            wall_ms: 0,
            counter: 0,
        })
    }

    /// Device id of this clock.
    pub fn device(&self) -> &str {
        &self.device
    }

    /// The last issued or observed timestamp.
    pub fn last(&self) -> Hlc {
        self.stamp()
    }

    /// Timestamp for a local event, using the system clock.
    pub fn now(&mut self) -> Hlc {
        self.tick(system_ms())
    }

    /// Timestamp for a local event at physical time `physical_ms`. Strictly
    /// greater than every timestamp issued or observed before.
    pub fn tick(&mut self, physical_ms: u64) -> Hlc {
        if physical_ms > self.wall_ms {
            self.wall_ms = physical_ms;
            self.counter = 0;
        } else {
            self.bump();
        }
        self.stamp()
    }

    /// Merges a timestamp received from another device, so the next local
    /// timestamp orders after it. Fails if `remote` is more than
    /// [`MAX_DRIFT_MS`] ahead of `physical_ms`; the clock is unchanged then.
    pub fn observe(&mut self, remote: &Hlc, physical_ms: u64) -> Result<Hlc, Error> {
        if remote.wall_ms > physical_ms.saturating_add(MAX_DRIFT_MS) {
            return Err(Error::ClockDrift {
                remote_ms: remote.wall_ms,
                local_ms: physical_ms,
            });
        }
        let wall = self.wall_ms.max(remote.wall_ms).max(physical_ms);
        let local_counter = (wall == self.wall_ms).then_some(self.counter);
        let remote_counter = (wall == remote.wall_ms).then_some(remote.counter);
        match local_counter.max(remote_counter) {
            Some(c) => {
                self.wall_ms = wall;
                self.counter = c;
                self.bump();
            }
            None => {
                self.wall_ms = wall;
                self.counter = 0;
            }
        }
        Ok(self.stamp())
    }

    /// Next logical step; on counter overflow moves to the next millisecond.
    fn bump(&mut self) {
        match self.counter.checked_add(1) {
            Some(c) => self.counter = c,
            None => {
                self.wall_ms += 1;
                self.counter = 0;
            }
        }
    }

    fn stamp(&self) -> Hlc {
        Hlc {
            wall_ms: self.wall_ms,
            counter: self.counter,
            device: self.device.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hlc(wall_ms: u64, counter: u32, device: &str) -> Hlc {
        Hlc {
            wall_ms,
            counter,
            device: device.into(),
        }
    }

    #[test]
    fn tick_is_monotonic_when_the_physical_clock_goes_back() {
        let mut c = HlcClock::new("a").unwrap();
        let t1 = c.tick(1000);
        let t2 = c.tick(900);
        let t3 = c.tick(1000);
        let t4 = c.tick(1001);
        assert!(t1 < t2 && t2 < t3 && t3 < t4);
        assert_eq!((t2.wall_ms, t2.counter), (1000, 1));
        assert_eq!((t4.wall_ms, t4.counter), (1001, 0));
    }

    #[test]
    fn observe_orders_after_the_remote_stamp() {
        let mut c = HlcClock::new("a").unwrap();
        c.tick(1000);
        let seen = c.observe(&hlc(2000, 7, "b"), 1500).unwrap();
        assert_eq!((seen.wall_ms, seen.counter), (2000, 8));
        let next = c.tick(1600);
        assert!(next > hlc(2000, 7, "b"));
        // Physical time ahead of both: counter resets.
        let later = c.observe(&hlc(2000, 9, "b"), 3000).unwrap();
        assert_eq!((later.wall_ms, later.counter), (3000, 0));
    }

    #[test]
    fn observe_rejects_a_clock_far_in_the_future() {
        let mut c = HlcClock::new("a").unwrap();
        c.tick(1000);
        let far = hlc(1000 + MAX_DRIFT_MS + 1, 0, "b");
        assert!(matches!(
            c.observe(&far, 1000),
            Err(Error::ClockDrift { .. })
        ));
        assert_eq!(c.last(), hlc(1000, 0, "a"));
    }

    #[test]
    fn counter_overflow_moves_to_the_next_millisecond() {
        let mut c = HlcClock::new("a").unwrap();
        c.observe(&hlc(5, u32::MAX - 1, "b"), 5).unwrap();
        let t = c.tick(5);
        assert_eq!((t.wall_ms, t.counter), (6, 0));
    }

    #[test]
    fn device_ids_are_checked() {
        assert!(HlcClock::new("tv-living_room1").is_ok());
        assert!(HlcClock::new("").is_err());
        assert!(HlcClock::new("a b").is_err());
        assert!(HlcClock::new("x".repeat(65)).is_err());
    }

    #[test]
    fn ties_are_broken_by_device() {
        assert!(hlc(1, 0, "a") < hlc(1, 0, "b"));
        assert!(hlc(1, 1, "a") > hlc(1, 0, "b"));
        assert_eq!(hlc(12, 3, "tv").to_string(), "12.3@tv");
    }
}
