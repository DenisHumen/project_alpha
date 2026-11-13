// SPDX-License-Identifier: MPL-2.0
//! `ProviderCache`: provider responses with a freshness TTL and a hard age
//! limit.
//!
//! Two clocks per entry: `expires_at` says when the entry should be
//! refreshed, but a *stale* entry is still served when the provider cannot
//! be reached (offline, no key, rate limited). Nothing is ever served or
//! kept past [`MAX_CACHE_AGE`] after it was fetched: TMDB's terms forbid
//! caching its data for more than six months (docs/legal/providers.md, TMDB
//! §6). [`ProviderCache::purge_older_than`] removes such entries, and
//! [`ProviderCache::purge_provider`] removes everything of one provider (for
//! when a licence ends).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};

/// Hard limit for keeping any provider response: 182 days, just under six
/// months.
pub const MAX_CACHE_AGE: Duration = Duration::from_secs(182 * 24 * 3600);

/// One cached response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    /// Response body as received.
    pub body: String,
    /// When it was fetched (Unix seconds).
    pub fetched_at: u64,
    /// When it should be refreshed (Unix seconds).
    pub expires_at: u64,
}

impl CacheEntry {
    /// Fresh: may be used without asking the provider.
    pub fn is_fresh(&self, now: u64) -> bool {
        now < self.expires_at && self.is_usable(now)
    }

    /// Within the hard age limit: may be used as a fallback.
    pub fn is_usable(&self, now: u64) -> bool {
        now.saturating_sub(self.fetched_at) < MAX_CACHE_AGE.as_secs()
    }
}

/// Cache failure (I/O, corrupt database). Clients treat it as a miss.
#[derive(Debug, thiserror::Error)]
#[error("provider cache: {0}")]
pub struct CacheError(pub String);

impl From<rusqlite::Error> for CacheError {
    fn from(e: rusqlite::Error) -> Self {
        Self(e.to_string())
    }
}

/// Storage for provider responses. Keys are credential-free.
pub trait ProviderCache: Send + Sync {
    /// The entry for `key`, fresh or not.
    fn get(&self, key: &str) -> Result<Option<CacheEntry>, CacheError>;
    /// Stores or replaces the entry for `key`.
    fn put(&self, provider: &str, key: &str, entry: &CacheEntry) -> Result<(), CacheError>;
    /// Deletes entries fetched before `unix_cutoff`; returns how many.
    fn purge_older_than(&self, unix_cutoff: u64) -> Result<usize, CacheError>;
    /// Deletes every entry of `provider`; returns how many.
    fn purge_provider(&self, provider: &str) -> Result<usize, CacheError>;
}

/// No caching at all.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoCache;

impl ProviderCache for NoCache {
    fn get(&self, _: &str) -> Result<Option<CacheEntry>, CacheError> {
        Ok(None)
    }
    fn put(&self, _: &str, _: &str, _: &CacheEntry) -> Result<(), CacheError> {
        Ok(())
    }
    fn purge_older_than(&self, _: u64) -> Result<usize, CacheError> {
        Ok(0)
    }
    fn purge_provider(&self, _: &str) -> Result<usize, CacheError> {
        Ok(0)
    }
}

/// In-process cache (tests, short-lived tools).
#[derive(Debug, Default)]
pub struct MemoryCache {
    entries: Mutex<HashMap<String, (String, CacheEntry)>>,
}

impl MemoryCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, (String, CacheEntry)>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

impl ProviderCache for MemoryCache {
    fn get(&self, key: &str) -> Result<Option<CacheEntry>, CacheError> {
        Ok(self.lock().get(key).map(|(_, e)| e.clone()))
    }

    fn put(&self, provider: &str, key: &str, entry: &CacheEntry) -> Result<(), CacheError> {
        self.lock()
            .insert(key.to_owned(), (provider.to_owned(), entry.clone()));
        Ok(())
    }

    fn purge_older_than(&self, unix_cutoff: u64) -> Result<usize, CacheError> {
        let mut map = self.lock();
        let before = map.len();
        map.retain(|_, (_, e)| e.fetched_at >= unix_cutoff);
        Ok(before - map.len())
    }

    fn purge_provider(&self, provider: &str) -> Result<usize, CacheError> {
        let mut map = self.lock();
        let before = map.len();
        map.retain(|_, (p, _)| p != provider);
        Ok(before - map.len())
    }
}

/// Persistent cache in its own SQLite file (in the platform cache
/// directory, not in the library database: it is disposable).
#[derive(Debug)]
pub struct SqliteCache {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS provider_cache (
    key        TEXT PRIMARY KEY NOT NULL,
    provider   TEXT NOT NULL,
    body       TEXT NOT NULL,
    fetched_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS provider_cache_fetched ON provider_cache (fetched_at);
CREATE INDEX IF NOT EXISTS provider_cache_provider ON provider_cache (provider);
";

impl SqliteCache {
    /// Opens or creates the cache database at `path`.
    pub fn open(path: &Path) -> Result<Self, CacheError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    /// A cache that lives in memory only.
    pub fn in_memory() -> Result<Self, CacheError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, CacheError> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn to_sql(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn from_sql(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

impl ProviderCache for SqliteCache {
    fn get(&self, key: &str) -> Result<Option<CacheEntry>, CacheError> {
        let conn = self.lock();
        let entry = conn
            .query_row(
                "SELECT body, fetched_at, expires_at FROM provider_cache WHERE key = ?1",
                params![key],
                |row| {
                    Ok(CacheEntry {
                        body: row.get(0)?,
                        fetched_at: from_sql(row.get(1)?),
                        expires_at: from_sql(row.get(2)?),
                    })
                },
            )
            .optional()?;
        Ok(entry)
    }

    fn put(&self, provider: &str, key: &str, entry: &CacheEntry) -> Result<(), CacheError> {
        self.lock().execute(
            "INSERT INTO provider_cache (key, provider, body, fetched_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (key) DO UPDATE SET provider = excluded.provider,
                 body = excluded.body, fetched_at = excluded.fetched_at,
                 expires_at = excluded.expires_at",
            params![
                key,
                provider,
                entry.body,
                to_sql(entry.fetched_at),
                to_sql(entry.expires_at)
            ],
        )?;
        Ok(())
    }

    fn purge_older_than(&self, unix_cutoff: u64) -> Result<usize, CacheError> {
        Ok(self.lock().execute(
            "DELETE FROM provider_cache WHERE fetched_at < ?1",
            params![to_sql(unix_cutoff)],
        )?)
    }

    fn purge_provider(&self, provider: &str) -> Result<usize, CacheError> {
        Ok(self.lock().execute(
            "DELETE FROM provider_cache WHERE provider = ?1",
            params![provider],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(fetched_at: u64, ttl: u64) -> CacheEntry {
        CacheEntry {
            body: format!("{{\"at\":{fetched_at}}}"),
            fetched_at,
            expires_at: fetched_at + ttl,
        }
    }

    fn contract(cache: &dyn ProviderCache) {
        assert_eq!(cache.get("a").unwrap(), None);
        cache.put("tmdb", "a", &entry(100, 10)).unwrap();
        cache.put("tmdb", "b", &entry(200, 10)).unwrap();
        cache.put("anilist", "c", &entry(50, 10)).unwrap();
        assert_eq!(cache.get("a").unwrap(), Some(entry(100, 10)));
        cache.put("tmdb", "a", &entry(150, 10)).unwrap();
        assert_eq!(cache.get("a").unwrap(), Some(entry(150, 10)));
        assert_eq!(cache.purge_older_than(151).unwrap(), 2);
        assert_eq!(cache.get("c").unwrap(), None);
        assert!(cache.get("b").unwrap().is_some());
        assert_eq!(cache.purge_provider("tmdb").unwrap(), 1);
        assert_eq!(cache.get("b").unwrap(), None);
    }

    #[test]
    fn memory_cache_contract() {
        contract(&MemoryCache::default());
    }

    #[test]
    fn sqlite_cache_contract() {
        contract(&SqliteCache::in_memory().unwrap());
    }

    #[test]
    fn sqlite_cache_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("provider-cache.sqlite");
        SqliteCache::open(&path)
            .unwrap()
            .put("tmdb", "k", &entry(1, 2))
            .unwrap();
        let reopened = SqliteCache::open(&path).unwrap();
        assert_eq!(reopened.get("k").unwrap(), Some(entry(1, 2)));
    }

    #[test]
    fn freshness_and_hard_age() {
        let e = entry(1_000, 100);
        assert!(e.is_fresh(1_050));
        assert!(!e.is_fresh(1_100));
        assert!(e.is_usable(1_100));
        let limit = 1_000 + MAX_CACHE_AGE.as_secs();
        assert!(e.is_usable(limit - 1));
        assert!(!e.is_usable(limit));
        // A TTL longer than the hard limit does not make an entry fresh.
        let long = entry(0, u64::MAX / 2);
        assert!(!long.is_fresh(MAX_CACHE_AGE.as_secs()));
    }
}
