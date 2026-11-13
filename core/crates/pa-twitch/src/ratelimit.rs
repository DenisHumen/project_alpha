// SPDX-License-Identifier: MPL-2.0
//! Token bucket that mirrors Helix `Ratelimit-*` headers (06 §1.4).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::transport::HttpResponse;

/// What the client knows about its bucket, from the last response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RateLimitState {
    /// Bucket size (`Ratelimit-Limit`), about 800 points per minute.
    pub limit: Option<u32>,
    /// Points left (`Ratelimit-Remaining`), decremented locally per request.
    pub remaining: Option<u32>,
    /// When the bucket refills (`Ratelimit-Reset`, Unix seconds).
    pub reset_at: Option<SystemTime>,
}

impl RateLimitState {
    /// How long to wait before the next request may be sent; `None` when a
    /// point is available (or nothing is known yet).
    pub fn wait_before_send(&self, now: SystemTime) -> Option<Duration> {
        if self.remaining != Some(0) {
            return None;
        }
        let reset = self.reset_at?;
        reset.duration_since(now).ok().filter(|d| !d.is_zero())
    }

    /// Accounts one request (each Helix call costs 1 point by default).
    pub fn consume(&mut self, now: SystemTime) {
        if let (Some(0), Some(reset)) = (self.remaining, self.reset_at)
            && now >= reset
        {
            // The bucket has refilled since; the exact amount comes with the
            // next response, until then assume a full bucket.
            self.remaining = self.limit;
        }
        if let Some(r) = self.remaining.as_mut() {
            *r = r.saturating_sub(1);
        }
    }

    /// Updates the state from response headers. Absent headers keep the old
    /// values.
    pub fn update(&mut self, response: &HttpResponse) {
        if let Some(limit) = header_u64(response, "ratelimit-limit") {
            self.limit = u32::try_from(limit).ok();
        }
        if let Some(remaining) = header_u64(response, "ratelimit-remaining") {
            self.remaining = u32::try_from(remaining).ok();
        }
        if let Some(reset) = header_u64(response, "ratelimit-reset") {
            self.reset_at = UNIX_EPOCH.checked_add(Duration::from_secs(reset));
        }
    }
}

/// Time to wait after a `429`: until `Ratelimit-Reset` if it is in the
/// future, otherwise `fallback`.
pub(crate) fn retry_after_429(
    response: &HttpResponse,
    now: SystemTime,
    fallback: Duration,
) -> Duration {
    header_u64(response, "ratelimit-reset")
        .and_then(|s| UNIX_EPOCH.checked_add(Duration::from_secs(s)))
        .and_then(|reset| reset.duration_since(now).ok())
        .filter(|d| !d.is_zero())
        .or_else(|| header_u64(response, "retry-after").map(Duration::from_secs))
        .unwrap_or(fallback)
}

fn header_u64(response: &HttpResponse, name: &str) -> Option<u64> {
    response.header(name)?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resp(headers: &[(&str, &str)]) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: Vec::new(),
        }
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn empty_bucket_waits_until_reset() {
        let mut state = RateLimitState::default();
        state.update(&resp(&[
            ("Ratelimit-Limit", "800"),
            ("Ratelimit-Remaining", "1"),
            ("Ratelimit-Reset", "1000"),
        ]));
        assert_eq!(state.wait_before_send(at(990)), None);
        state.consume(at(990));
        assert_eq!(state.remaining, Some(0));
        assert_eq!(state.wait_before_send(at(990)), Some(Duration::from_secs(10)));
        assert_eq!(state.wait_before_send(at(1000)), None);
        state.consume(at(1000));
        assert_eq!(state.remaining, Some(799));
    }

    #[test]
    fn unknown_state_never_waits() {
        assert_eq!(RateLimitState::default().wait_before_send(at(0)), None);
    }

    #[test]
    fn retry_after_prefers_reset_then_retry_after_then_fallback() {
        let fb = Duration::from_secs(1);
        let r = resp(&[("Ratelimit-Reset", "105")]);
        assert_eq!(retry_after_429(&r, at(100), fb), Duration::from_secs(5));
        let r = resp(&[("Ratelimit-Reset", "50"), ("Retry-After", "3")]);
        assert_eq!(retry_after_429(&r, at(100), fb), Duration::from_secs(3));
        assert_eq!(retry_after_429(&resp(&[]), at(100), fb), fb);
    }
}
