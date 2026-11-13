// SPDX-License-Identifier: MPL-2.0
//! Runtime download of mapping lists **directly from upstream** onto the
//! user's device, with ETag, integrity checks, a local cache and atomic
//! replacement.
//!
//! Anime-Lists and Fribb have no licence, so the project never hosts,
//! mirrors or proxies them: the URLs below point at the upstream repositories
//! and are fixed in code (`docs/legal/anime-mappings.md` §3–4). Requests go
//! no more than once per [`DEFAULT_MIN_INTERVAL_SECS`] per source.
//!
//! Network I/O is the caller's: implement [`HttpClient`] (server and clients
//! use their own HTTP stack). Time is passed in, so the logic is testable
//! offline.
//!
//! Cache layout in `dir`:
//! - `manifest.json` — per source: URL, file name, SHA-256, ETag, timestamps;
//! - `<name>.<sha256-prefix>.<ext>` — content-addressed data files.
//!
//! An update writes the new data file first, then swaps `manifest.json` with
//! a single rename, then deletes the old file. Readers that go through
//! [`Updater::load`] see either the old or the new version, never a mix, and
//! a corrupted file is detected by its SHA-256.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Result, anime_lists, fribb, offline_db};

/// Minimum time between two requests for the same source (24 h).
pub const DEFAULT_MIN_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Upstream data source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// anime-offline-database (ODbL). Updates of the bundled snapshot.
    OfflineDatabase,
    /// Anime-Lists `anime-list-master.xml` (no licence: device download only).
    AnimeLists,
    /// Fribb `anime-list-full.json` (no licence: device download only).
    Fribb,
}

/// Static description of a source.
#[derive(Debug, Clone, Copy)]
pub struct SourceSpec {
    /// Stable key in the manifest.
    pub key: &'static str,
    /// Upstream URL. Never a project-hosted mirror.
    pub url: &'static str,
    pub extension: &'static str,
    /// Licence permits bundling/mirroring by the project.
    pub redistributable: bool,
    /// Downloads with fewer entries are rejected as truncated or broken.
    pub min_entries: usize,
    /// Parses the body and returns the entry count.
    pub validate: fn(&[u8]) -> Result<usize>,
}

impl Source {
    pub const ALL: [Source; 3] = [Source::OfflineDatabase, Source::AnimeLists, Source::Fribb];

    pub fn spec(self) -> SourceSpec {
        match self {
            Source::OfflineDatabase => SourceSpec {
                key: "anime-offline-database",
                url: "https://github.com/cedya77/anime-offline-database/releases/download/latest/anime-offline-database.jsonl",
                extension: "jsonl",
                redistributable: true,
                min_entries: 20_000,
                validate: offline_db::validate_upstream,
            },
            Source::AnimeLists => SourceSpec {
                key: "anime-lists",
                url: "https://raw.githubusercontent.com/Anime-Lists/anime-lists/master/anime-list-master.xml",
                extension: "xml",
                redistributable: false,
                min_entries: 5_000,
                validate: anime_lists::validate,
            },
            Source::Fribb => SourceSpec {
                key: "fribb-anime-list-full",
                url: "https://raw.githubusercontent.com/Fribb/anime-lists/master/anime-list-full.json",
                extension: "json",
                redistributable: false,
                min_entries: 10_000,
                validate: fribb::validate,
            },
        }
    }
}

/// Minimal HTTP response the updater needs.
#[derive(Debug, Clone, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub etag: Option<String>,
    pub body: Vec<u8>,
}

/// HTTP GET implemented by the caller. Must follow redirects within the
/// upstream host (GitHub release assets redirect to its CDN) and send an
/// `If-None-Match` header when `if_none_match` is set.
pub trait HttpClient {
    fn get(
        &self,
        url: &str,
        if_none_match: Option<&str>,
    ) -> std::result::Result<HttpResponse, String>;
}

/// Cached state of one source.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    pub url: String,
    pub file: String,
    pub sha256: String,
    #[serde(default)]
    pub etag: Option<String>,
    pub entries: usize,
    /// Unix seconds of the last successful download.
    pub fetched_at: u64,
    /// Unix seconds of the last request (200 or 304).
    pub checked_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    sources: BTreeMap<String, CacheEntry>,
}

/// Result of [`Updater::refresh`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Checked recently; no request made.
    Skipped,
    /// Server answered 304.
    NotModified,
    /// New data validated and installed.
    Updated { entries: usize },
}

/// Download cache for mapping sources.
#[derive(Debug, Clone)]
pub struct Updater {
    dir: PathBuf,
    pub min_interval_secs: u64,
    /// Overrides [`SourceSpec::min_entries`] (tests, small fixtures).
    pub min_entries_override: Option<usize>,
}

/// Lower-case hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

impl Updater {
    /// Cache in `dir` (created if missing), e.g. the app's data directory.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            min_interval_secs: DEFAULT_MIN_INTERVAL_SECS,
            min_entries_override: None,
        }
    }

    fn manifest_path(&self) -> PathBuf {
        self.dir.join("manifest.json")
    }

    fn read_manifest(&self) -> Result<Manifest> {
        match fs::read(self.manifest_path()) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn write_manifest(&self, m: &Manifest) -> Result<()> {
        write_atomic(&self.manifest_path(), &serde_json::to_vec_pretty(m)?)
    }

    /// Cached state of `source`, if any.
    pub fn status(&self, source: Source) -> Result<Option<CacheEntry>> {
        Ok(self.read_manifest()?.sources.remove(source.spec().key))
    }

    /// Cached bytes of `source`, verified against the manifest SHA-256.
    /// `Ok(None)` if nothing is cached yet.
    pub fn load(&self, source: Source) -> Result<Option<Vec<u8>>> {
        let Some(entry) = self.status(source)? else {
            return Ok(None);
        };
        let bytes = fs::read(self.dir.join(&entry.file))?;
        let actual = sha256_hex(&bytes);
        if actual != entry.sha256 {
            return Err(Error::Invalid(format!(
                "{}: checksum mismatch (expected {}, got {actual})",
                entry.file, entry.sha256
            )));
        }
        Ok(Some(bytes))
    }

    /// Check upstream for a newer version of `source` and install it.
    ///
    /// On any error the previously cached version stays in place.
    pub fn refresh(
        &self,
        client: &dyn HttpClient,
        source: Source,
        now: u64,
        force: bool,
    ) -> Result<Outcome> {
        let spec = source.spec();
        fs::create_dir_all(&self.dir)?;
        let mut manifest = self.read_manifest()?;
        // A cached copy that fails its checksum must not be revalidated by ETag.
        let prev = manifest
            .sources
            .get(spec.key)
            .filter(|e| e.url == spec.url)
            .cloned()
            .filter(|_| self.load(source).is_ok_and(|b| b.is_some()));
        if let Some(p) = &prev
            && !force
            && now < p.checked_at.saturating_add(self.min_interval_secs)
        {
            return Ok(Outcome::Skipped);
        }
        let etag = prev.as_ref().and_then(|p| p.etag.as_deref());
        let resp = client.get(spec.url, etag).map_err(Error::Http)?;
        match resp.status {
            304 if prev.is_some() => {
                let mut p = prev.unwrap_or_default();
                p.checked_at = now;
                manifest.sources.insert(spec.key.into(), p);
                self.write_manifest(&manifest)?;
                Ok(Outcome::NotModified)
            }
            200 => {
                let entries = (spec.validate)(&resp.body)?;
                let min = self.min_entries_override.unwrap_or(spec.min_entries);
                if entries < min {
                    return Err(Error::Invalid(format!(
                        "{}: {entries} entries, expected at least {min}",
                        spec.key
                    )));
                }
                if let Some(p) = &prev
                    && entries < p.entries / 2
                {
                    return Err(Error::Invalid(format!(
                        "{}: {entries} entries, less than half of cached {}",
                        spec.key, p.entries
                    )));
                }
                let sha = sha256_hex(&resp.body);
                let file = format!("{}.{}.{}", spec.key, &sha[..16], spec.extension);
                write_atomic(&self.dir.join(&file), &resp.body)?;
                let old_file = manifest
                    .sources
                    .get(spec.key)
                    .map(|e| e.file.clone())
                    .filter(|f| *f != file);
                manifest.sources.insert(
                    spec.key.into(),
                    CacheEntry {
                        url: spec.url.into(),
                        file,
                        sha256: sha,
                        etag: resp.etag,
                        entries,
                        fetched_at: now,
                        checked_at: now,
                    },
                );
                self.write_manifest(&manifest)?;
                if let Some(old) = old_file {
                    let _ = fs::remove_file(self.dir.join(old));
                }
                Ok(Outcome::Updated { entries })
            }
            s => Err(Error::Http(format!(
                "{}: unexpected HTTP status {s}",
                spec.url
            ))),
        }
    }
}
