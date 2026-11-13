// SPDX-License-Identifier: MPL-2.0
//! In-memory TTL cache with the Developer Agreement's 24-hour ceiling.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, SystemTime};

/// Longest time Twitch data may be cached (Developer Services Agreement,
/// docs/legal/providers.md §2.4). Every TTL in this crate is clamped to it.
pub const MAX_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Bounded map whose entries expire after a TTL (never above
/// [`MAX_CACHE_TTL`]). Time is passed in, so it works with any [`crate::Clock`].
#[derive(Debug)]
pub struct TtlCache<K, V> {
    ttl: Duration,
    capacity: usize,
    entries: HashMap<K, (SystemTime, V)>,
}

impl<K: Eq + Hash + Clone, V: Clone> TtlCache<K, V> {
    /// New cache; `ttl` is clamped to [`MAX_CACHE_TTL`], `capacity` to ≥ 1.
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl: ttl.min(MAX_CACHE_TTL),
            capacity: capacity.max(1),
            entries: HashMap::new(),
        }
    }

    /// Effective TTL after clamping.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Fresh value for `key`, if any.
    pub fn get(&self, key: &K, now: SystemTime) -> Option<V> {
        let (stored, value) = self.entries.get(key)?;
        let age = now.duration_since(*stored).unwrap_or_default();
        (age < self.ttl).then(|| value.clone())
    }

    /// Stores `value`. When full, expired entries go first, then the oldest.
    pub fn insert(&mut self, key: K, value: V, now: SystemTime) {
        if self.ttl.is_zero() {
            return;
        }
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&key) {
            let ttl = self.ttl;
            self.entries
                .retain(|_, (t, _)| now.duration_since(*t).unwrap_or_default() < ttl);
            if self.entries.len() >= self.capacity
                && let Some(oldest) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, (t, _))| *t)
                    .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(key, (now, value));
    }

    /// Drops everything (e.g. on sign-out, when Twitch requires deleting the
    /// user's data).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Number of stored entries, fresh or not.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when nothing is stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: SystemTime = SystemTime::UNIX_EPOCH;

    #[test]
    fn ttl_is_clamped_to_24h() {
        let cache: TtlCache<u8, u8> = TtlCache::new(Duration::from_secs(7 * 24 * 3600), 4);
        assert_eq!(cache.ttl(), MAX_CACHE_TTL);
    }

    #[test]
    fn entries_expire() {
        let mut cache = TtlCache::new(Duration::from_secs(60), 4);
        cache.insert("a", 1, T0);
        assert_eq!(cache.get(&"a", T0 + Duration::from_secs(59)), Some(1));
        assert_eq!(cache.get(&"a", T0 + Duration::from_secs(60)), None);
    }

    #[test]
    fn evicts_oldest_when_full() {
        let mut cache = TtlCache::new(Duration::from_secs(600), 2);
        cache.insert(1, 'a', T0);
        cache.insert(2, 'b', T0 + Duration::from_secs(1));
        cache.insert(3, 'c', T0 + Duration::from_secs(2));
        let now = T0 + Duration::from_secs(3);
        assert_eq!(cache.get(&1, now), None);
        assert_eq!(cache.get(&2, now), Some('b'));
        assert_eq!(cache.get(&3, now), Some('c'));
        assert_eq!(cache.len(), 2);
    }
}
