// SPDX-License-Identifier: MPL-2.0
//! Client-side rate limiting and retry backoff.
//!
//! [`RateLimiter`] is a token bucket shared by all threads of one client:
//! it keeps us under the provider's published limit before the provider has
//! to say no. When it says no anyway (HTTP 429), [`RetryPolicy`] decides how
//! long to wait, and the limiter is paused for everyone until then.

use std::sync::Mutex;
use std::time::Duration;

use crate::clock::Clock;

/// Token bucket parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimit {
    /// Requests that may go out back to back.
    pub burst: u32,
    /// Sustained requests per second.
    pub per_second: f64,
}

/// Shared token bucket.
#[derive(Debug)]
pub struct RateLimiter {
    limit: RateLimit,
    state: Mutex<BucketState>,
}

#[derive(Debug)]
struct BucketState {
    tokens: f64,
    last: Option<Duration>,
    paused_until: Duration,
}

impl RateLimiter {
    /// A full bucket.
    pub fn new(limit: RateLimit) -> Self {
        let limit = RateLimit {
            burst: limit.burst.max(1),
            per_second: if limit.per_second > 0.0 {
                limit.per_second
            } else {
                1.0
            },
        };
        Self {
            limit,
            state: Mutex::new(BucketState {
                tokens: f64::from(limit.burst),
                last: None,
                paused_until: Duration::ZERO,
            }),
        }
    }

    /// Takes one token, sleeping on `clock` until one is available and any
    /// pause has passed. Callers are served one at a time.
    pub fn acquire(&self, clock: &dyn Clock) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = clock.monotonic();
        if s.paused_until > now {
            clock.sleep(s.paused_until - now);
        }
        let now = clock.monotonic();
        if let Some(last) = s.last {
            let refill = (now.saturating_sub(last)).as_secs_f64() * self.limit.per_second;
            s.tokens = (s.tokens + refill).min(f64::from(self.limit.burst));
        }
        s.last = Some(now);
        if s.tokens < 1.0 {
            let wait = Duration::from_secs_f64((1.0 - s.tokens) / self.limit.per_second);
            clock.sleep(wait);
            s.last = Some(clock.monotonic());
            s.tokens = 1.0;
        }
        s.tokens -= 1.0;
    }

    /// Holds every caller until `delay` from now (after a 429).
    pub fn pause_for(&self, clock: &dyn Clock, delay: Duration) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let until = clock.monotonic() + delay;
        if until > s.paused_until {
            s.paused_until = until;
        }
        // Restart slowly after the pause instead of with a full burst.
        s.tokens = 0.0;
        s.last = Some(s.paused_until);
    }
}

/// Retries for HTTP 429 and transient 5xx responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt.
    pub max_retries: u32,
    /// Delay before the first retry; doubles each time.
    pub base_delay: Duration,
    /// Upper bound for one delay, also for a provider's `Retry-After`.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 4,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// Delay before retry number `retry` (0-based): the provider's
    /// `Retry-After` if given, else exponential; both capped by `max_delay`.
    pub fn delay(&self, retry: u32, retry_after: Option<Duration>) -> Duration {
        let exponential = self
            .base_delay
            .saturating_mul(2u32.saturating_pow(retry.min(16)));
        retry_after.unwrap_or(exponential).min(self.max_delay)
    }
}

/// Parses `Retry-After` in its delta-seconds form. The HTTP-date form is
/// ignored (exponential backoff is used instead).
pub fn parse_retry_after(value: Option<&str>) -> Option<Duration> {
    value?.trim().parse::<u64>().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    #[test]
    fn burst_then_steady_rate() {
        let clock = ManualClock::default();
        let limiter = RateLimiter::new(RateLimit {
            burst: 3,
            per_second: 10.0,
        });
        for _ in 0..3 {
            limiter.acquire(&clock);
        }
        assert!(clock.sleeps().is_empty(), "burst goes out without waiting");
        limiter.acquire(&clock);
        limiter.acquire(&clock);
        let sleeps = clock.sleeps();
        assert_eq!(sleeps.len(), 2);
        for s in sleeps {
            assert!((s.as_secs_f64() - 0.1).abs() < 1e-6, "{s:?}");
        }
    }

    #[test]
    fn refills_over_time() {
        let clock = ManualClock::default();
        let limiter = RateLimiter::new(RateLimit {
            burst: 1,
            per_second: 2.0,
        });
        limiter.acquire(&clock);
        clock.advance(Duration::from_secs(1));
        limiter.acquire(&clock);
        assert!(clock.sleeps().is_empty());
    }

    #[test]
    fn pause_holds_next_caller() {
        let clock = ManualClock::default();
        let limiter = RateLimiter::new(RateLimit {
            burst: 10,
            per_second: 10.0,
        });
        limiter.pause_for(&clock, Duration::from_secs(5));
        limiter.acquire(&clock);
        assert_eq!(clock.sleeps()[0], Duration::from_secs(5));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay(0, None), Duration::from_secs(1));
        assert_eq!(p.delay(3, None), Duration::from_secs(8));
        assert_eq!(p.delay(10, None), Duration::from_secs(30));
        assert_eq!(
            p.delay(0, Some(Duration::from_secs(7))),
            Duration::from_secs(7)
        );
        assert_eq!(
            p.delay(0, Some(Duration::from_secs(600))),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn retry_after_seconds_only() {
        assert_eq!(parse_retry_after(Some(" 3 ")), Some(Duration::from_secs(3)));
        assert_eq!(
            parse_retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
            None
        );
        assert_eq!(parse_retry_after(None), None);
    }
}
