// SPDX-License-Identifier: MPL-2.0
//! [`RemoteConfigClient`]: built-in copy → verified disk cache → refresh
//! from the public repo with `If-None-Match`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cache::{CacheEntry, DiskCache};
use crate::fetch::ConfigFetcher;
use crate::model::RemoteConfig;
use crate::verify::TrustedKey;
use crate::{BUILTIN_JSON, Error};

/// File name of the main document in the repo's `config/` folder. It is
/// also the name the signature's trusted comment must carry.
pub const DEFAULT_FILE_NAME: &str = "remote-config.json";

/// Where a document and its signature are published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub config_url: String,
    pub signature_url: String,
}

impl Endpoint {
    /// `<base>/<file>` and `<base>/<file>.minisig`.
    pub fn new(base: &str, file_name: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            config_url: format!("{base}/{file_name}"),
            signature_url: format!("{base}/{file_name}.minisig"),
        }
    }
}

/// Static hosting of the public repo, tried in order: raw files of `main`,
/// then GitHub Pages (once the repo publishes `config/` there). Both are
/// plain static files: no project server (ADR-0004).
pub fn default_endpoints(file_name: &str) -> Vec<Endpoint> {
    vec![
        Endpoint::new(
            "https://raw.githubusercontent.com/DenisHumen/project_alpha/main/config",
            file_name,
        ),
        Endpoint::new(
            "https://denishumen.github.io/project_alpha/config",
            file_name,
        ),
    ]
}

/// Settings of a client.
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// Directory of the disk cache (the platform cache or app-data dir).
    pub cache_dir: PathBuf,
    /// Document name, see [`DEFAULT_FILE_NAME`].
    pub file_name: String,
    /// Mirrors tried in order on every refresh.
    pub endpoints: Vec<Endpoint>,
}

impl ClientOptions {
    /// The main document from [`default_endpoints`], cached in `cache_dir`.
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            file_name: DEFAULT_FILE_NAME.to_owned(),
            endpoints: default_endpoints(DEFAULT_FILE_NAME),
        }
    }
}

/// Where the current document came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// Compiled into the app.
    BuiltIn,
    /// Last valid copy from disk (loaded at start, no network yet).
    Cache,
    /// Downloaded and verified in this run.
    Remote,
}

/// The current document and where it came from.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub config: Arc<RemoteConfig>,
    pub source: ConfigSource,
    /// Unix seconds of the last successful check, if any.
    pub checked_at: Option<u64>,
    /// Unix seconds of the last failed refresh after it, if any.
    pub failed_at: Option<u64>,
}

/// Result of a refresh that did not fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// A newer document was verified, cached and is now current.
    Updated { serial: u64 },
    /// The server answered `304`, or sent the same serial again.
    Unchanged,
    /// [`RemoteConfigClient::refresh_if_due`] only: not time yet.
    NotDue,
}

struct State {
    current: Arc<RemoteConfig>,
    source: ConfigSource,
    entry: Option<CacheEntry>,
    checked_at: Option<u64>,
    failed_at: Option<u64>,
}

/// Remote config client. Cheap to share (`Arc`); every method is blocking
/// and thread-safe, refreshes are serialized.
pub struct RemoteConfigClient {
    key: Option<TrustedKey>,
    options: ClientOptions,
    fetcher: Arc<dyn ConfigFetcher>,
    cache: DiskCache,
    state: Mutex<State>,
    refreshing: Mutex<()>,
}

impl RemoteConfigClient {
    /// Opens the client with the app's built-in copy. Never touches the
    /// network. `key == None` (production key still a placeholder): the
    /// disk cache is ignored and refresh is disabled — built-in copy only.
    pub fn open(
        key: Option<TrustedKey>,
        options: ClientOptions,
        fetcher: Arc<dyn ConfigFetcher>,
    ) -> Self {
        let builtin = RemoteConfig::parse(BUILTIN_JSON.as_bytes())
            .expect("built-in remote config is checked by the crate's tests");
        Self::open_with_builtin(key, options, fetcher, builtin)
    }

    /// [`Self::open`] with a given built-in document (tests, other files).
    pub fn open_with_builtin(
        key: Option<TrustedKey>,
        options: ClientOptions,
        fetcher: Arc<dyn ConfigFetcher>,
        builtin: RemoteConfig,
    ) -> Self {
        let cache = DiskCache::new(&options.cache_dir, &options.file_name);
        let mut state = State {
            current: Arc::new(builtin),
            source: ConfigSource::BuiltIn,
            entry: None,
            checked_at: None,
            failed_at: None,
        };
        if let Some(key) = &key
            && let Some(entry) = cache.load()
            && let Ok(config) = check(key, &options.file_name, &entry.body, &entry.signature)
            // A cache older than the built-in copy (app was updated) loses.
            && config.serial >= state.current.serial
        {
            state.current = Arc::new(config);
            state.source = ConfigSource::Cache;
            state.checked_at = Some(entry.checked_at);
            state.entry = Some(entry);
        }
        Self {
            key,
            options,
            fetcher,
            cache,
            state: Mutex::new(state),
            refreshing: Mutex::new(()),
        }
    }

    /// The current document.
    pub fn config(&self) -> Arc<RemoteConfig> {
        self.lock().current.clone()
    }

    /// The current document with its origin and check times.
    pub fn snapshot(&self) -> Snapshot {
        let s = self.lock();
        Snapshot {
            config: s.current.clone(),
            source: s.source,
            checked_at: s.checked_at,
            failed_at: s.failed_at,
        }
    }

    /// Can this client ever refresh (is there a trusted key)?
    pub fn remote_enabled(&self) -> bool {
        self.key.is_some()
    }

    /// Is a refresh due at `now` (unix seconds) under the current
    /// document's [`RefreshPolicy`](crate::RefreshPolicy)?
    pub fn refresh_due(&self, now: u64) -> bool {
        if self.key.is_none() {
            return false;
        }
        let s = self.lock();
        let policy = s.current.refresh;
        match (s.checked_at, s.failed_at) {
            (_, Some(failed)) => now >= failed.saturating_add(policy.retry()),
            (Some(checked), None) => now >= checked.saturating_add(policy.interval()),
            (None, None) => true,
        }
    }

    /// [`Self::refresh`] if [`Self::refresh_due`], else `NotDue`.
    pub fn refresh_if_due(&self) -> Result<RefreshOutcome, Error> {
        let now = now_secs();
        if !self.refresh_due(now) {
            return Ok(RefreshOutcome::NotDue);
        }
        self.refresh_at(now)
    }

    /// Checks the mirrors for a newer document now.
    pub fn refresh(&self) -> Result<RefreshOutcome, Error> {
        self.refresh_at(now_secs())
    }

    /// [`Self::refresh`] with an explicit clock (unix seconds).
    ///
    /// On any error the current document stays as it is and the disk cache
    /// is not touched; the next attempt is due after the retry delay.
    pub fn refresh_at(&self, now: u64) -> Result<RefreshOutcome, Error> {
        let key = self.key.as_ref().ok_or(Error::NoTrustedKey)?;
        let _one_at_a_time = self
            .refreshing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut last_error = Error::Offline("no endpoints configured".into());
        for endpoint in &self.options.endpoints {
            match self.try_endpoint(key, endpoint, now) {
                Ok(outcome) => {
                    let mut s = self.lock();
                    s.checked_at = Some(now);
                    s.failed_at = None;
                    return Ok(outcome);
                }
                // A rollback attempt is not a mirror problem: stop here.
                Err(e @ Error::Rollback { .. }) => {
                    last_error = e;
                    break;
                }
                // Offline, HTTP errors and bad signatures (e.g. a CDN that
                // still serves the old `.minisig` next to a new file) may be
                // fine on the next mirror.
                Err(e) => last_error = e,
            }
        }
        self.lock().failed_at = Some(now);
        Err(last_error)
    }

    fn try_endpoint(
        &self,
        key: &TrustedKey,
        endpoint: &Endpoint,
        now: u64,
    ) -> Result<RefreshOutcome, Error> {
        let cached = self
            .lock()
            .entry
            .clone()
            .filter(|e| e.url == endpoint.config_url);
        let etag = cached.as_ref().and_then(|e| e.etag.clone());
        let response = self
            .fetcher
            .get(&endpoint.config_url, etag.as_deref())
            .map_err(|e| Error::Offline(e.0))?;
        match (response.status, cached) {
            (304, Some(mut entry)) if etag.is_some() => {
                entry.checked_at = now;
                // Only the check time changes; a failed write costs one
                // extra download next start, not correctness.
                let _ = self.cache.store(&entry);
                self.lock().entry = Some(entry);
                return Ok(RefreshOutcome::Unchanged);
            }
            (200, _) => {}
            (status, _) => return Err(Error::Http { status }),
        }
        let body = String::from_utf8(response.body)
            .map_err(|_| Error::Invalid("config is not UTF-8".into()))?;
        let sig = self
            .fetcher
            .get(&endpoint.signature_url, None)
            .map_err(|e| Error::Offline(e.0))?;
        if sig.status != 200 {
            return Err(Error::Http { status: sig.status });
        }
        let signature = String::from_utf8(sig.body)
            .map_err(|_| Error::BadSignature("signature is not UTF-8".into()))?;
        let config = check(key, &self.options.file_name, &body, &signature)?;

        let have = self.lock().current.serial;
        if config.serial < have {
            return Err(Error::Rollback {
                got: config.serial,
                have,
            });
        }
        let entry = CacheEntry {
            url: endpoint.config_url.clone(),
            etag: response.etag,
            body,
            signature,
            checked_at: now,
        };
        self.cache.store(&entry)?;
        let mut s = self.lock();
        let outcome = if config.serial == have && s.source != ConfigSource::BuiltIn {
            RefreshOutcome::Unchanged
        } else {
            RefreshOutcome::Updated {
                serial: config.serial,
            }
        };
        s.current = Arc::new(config);
        s.source = ConfigSource::Remote;
        s.entry = Some(entry);
        Ok(outcome)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Signature first, then parsing: unsigned bytes are never parsed.
fn check(
    key: &TrustedKey,
    file_name: &str,
    body: &str,
    signature: &str,
) -> Result<RemoteConfig, Error> {
    key.verify(file_name, body.as_bytes(), signature)?;
    RemoteConfig::parse(body.as_bytes())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
