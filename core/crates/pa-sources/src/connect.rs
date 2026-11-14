// SPDX-License-Identifier: MPL-2.0
//! Building the [`Vfs`] backend of a source, the reachability check and
//! the speed test.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use pa_secrets::SecretString;
use pa_vfs::{EntryKind, LocalFs, Vfs, VfsError, VfsPath};
use pa_vfs_smb::{SmbConfig, SmbFs};
use pa_vfs_webdav::{TlsTrust, WebDavConfig, WebDavFs};

use crate::manager::Stored;
use crate::model::SpeedSample;
use crate::{
    Error, Result, SmbAddress, SmbEncryption, SmbVersion, SourceKind, SourceManager,
    SourceStatus, file_uri_to_path, now_ms, scrub,
};

/// Limits of [`SourceManager::speed_test`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeedTestOptions {
    /// Stop after reading this many bytes (default 32 MiB).
    pub max_bytes: u64,
    /// Stop reading after this long (default 5 s).
    pub max_duration: Duration,
    /// Size of one read (default 1 MiB).
    pub chunk_size: usize,
    /// Directories listed while looking for a file to read (default 32).
    pub max_dirs: usize,
}

impl Default for SpeedTestOptions {
    fn default() -> Self {
        Self {
            max_bytes: 32 * 1024 * 1024,
            max_duration: Duration::from_secs(5),
            chunk_size: 1024 * 1024,
            max_dirs: 32,
        }
    }
}

/// Result of [`SourceManager::speed_test`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeedReport {
    /// Round trip of one metadata request on a warm connection.
    pub latency_ms: u64,
    /// The file that was read; `None` if the source has no files.
    pub file: Option<VfsPath>,
    /// Bytes read.
    pub bytes_read: u64,
    /// Time spent reading.
    pub elapsed: Duration,
    /// Sequential throughput; `None` if nothing was read.
    pub bytes_per_sec: Option<u64>,
}

/// Maps a backend error to [`Error::Source`] with secrets masked.
fn source_error(id: i64, e: &VfsError, password: Option<&SecretString>) -> Error {
    Error::Source {
        id,
        kind: e.kind(),
        message: scrub(e.message(), password),
    }
}

/// Builds the backend; does not touch the network.
fn backend(stored: &Stored, password: Option<SecretString>) -> pa_vfs::Result<Box<dyn Vfs>> {
    let s = &stored.source;
    let o = &s.options;
    let invalid = |m: String| VfsError::new(pa_vfs::ErrorKind::InvalidPath, m);
    match s.kind {
        SourceKind::Local => {
            let root = file_uri_to_path(&s.uri)
                .ok_or_else(|| invalid(format!("not a file URI: {}", s.uri)))?;
            Ok(Box::new(LocalFs::new(root)?))
        }
        SourceKind::Smb => {
            let a = SmbAddress::parse(&s.uri).map_err(|e| invalid(e.to_string()))?;
            let mut c = SmbConfig::new(a.server, a.share)
                .with_root(&a.folder)
                .with_version(match o.smb_version {
                    SmbVersion::Any => pa_vfs_smb::Version::Any,
                    SmbVersion::Smb2 => pa_vfs_smb::Version::Any2,
                    SmbVersion::Smb3 => pa_vfs_smb::Version::Any3,
                })
                .with_signing(if o.smb_signing_required {
                    pa_vfs_smb::Signing::Required
                } else {
                    pa_vfs_smb::Signing::Auto
                })
                .with_encryption(match o.smb_encryption {
                    SmbEncryption::Auto => pa_vfs_smb::Encryption::Auto,
                    SmbEncryption::Required => pa_vfs_smb::Encryption::Required,
                    SmbEncryption::Off => pa_vfs_smb::Encryption::Off,
                });
            if let Some(user) = &s.username {
                let pass = password.as_ref().map_or("", SecretString::expose);
                let mut creds = pa_vfs_smb::Credentials::new(user.as_str(), pass);
                if let Some(domain) = &o.smb_domain {
                    creds = creds.with_domain(domain.as_str());
                }
                c = c.with_credentials(creds);
            }
            if let Some(t) = o.timeout_secs {
                c = c.with_timeout(Duration::from_secs(t.into()));
            }
            if let Some(b) = o.read_ahead_bytes {
                c = c.with_read_ahead(usize::try_from(b).unwrap_or(usize::MAX));
            }
            Ok(Box::new(SmbFs::new(c)?))
        }
        SourceKind::WebDav => {
            let mut c = WebDavConfig::new(s.uri.as_str());
            if let Some(user) = &s.username {
                let pass = password.as_ref().map_or("", SecretString::expose);
                c = c.with_credentials(pa_vfs_webdav::Credentials::new(user.as_str(), pass));
            }
            if let Some(pin) = &o.tls_pin {
                let fp = pin.parse().map_err(|e| invalid(format!("{e}")))?;
                c = c.with_tls(TlsTrust::Pinned(fp));
            }
            if let Some(t) = o.timeout_secs {
                c.timeout = Duration::from_secs(t.into());
            }
            if let Some(b) = o.read_ahead_bytes {
                c = c.with_read_ahead(usize::try_from(b).unwrap_or(usize::MAX));
            }
            Ok(Box::new(WebDavFs::new(c)?))
        }
    }
}

/// Looks for the largest file in the first `max_dirs` directories
/// (breadth-first), stopping early at one of at least `enough` bytes.
fn find_file(vfs: &dyn Vfs, max_dirs: usize, enough: u64) -> pa_vfs::Result<Option<VfsPath>> {
    let mut queue = VecDeque::from([VfsPath::root()]);
    let mut best: Option<(u64, VfsPath)> = None;
    let mut listed = 0;
    while let Some(dir) = queue.pop_front() {
        if listed >= max_dirs {
            break;
        }
        listed += 1;
        let entries = match vfs.list(&dir) {
            Ok(entries) => entries,
            // The root must be readable; a subfolder may be denied.
            Err(e) if dir.is_root() => return Err(e),
            Err(_) => continue,
        };
        for e in entries {
            match e.metadata.kind {
                EntryKind::Dir => queue.push_back(e.path),
                EntryKind::File if e.metadata.size > best.as_ref().map_or(0, |b| b.0) => {
                    let size = e.metadata.size;
                    best = Some((size, e.path));
                    if size >= enough {
                        return Ok(best.map(|b| b.1));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(best.map(|b| b.1))
}

fn measure(vfs: &dyn Vfs, limits: &SpeedTestOptions) -> pa_vfs::Result<SpeedReport> {
    // First request opens the connection; the second one is the latency.
    vfs.stat(&VfsPath::root())?;
    let start = Instant::now();
    vfs.stat(&VfsPath::root())?;
    let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    let file = find_file(vfs, limits.max_dirs.max(1), limits.max_bytes)?;
    let mut bytes_read = 0u64;
    let mut elapsed = Duration::ZERO;
    if let Some(path) = &file {
        let source = vfs.open(path)?;
        let mut buf = vec![0u8; limits.chunk_size.max(4096)];
        let start = Instant::now();
        while bytes_read < limits.max_bytes.min(source.size())
            && start.elapsed() < limits.max_duration
        {
            let n = source.read_at(bytes_read, &mut buf)?;
            if n == 0 {
                break;
            }
            bytes_read += n as u64;
        }
        elapsed = start.elapsed();
    }
    let bytes_per_sec = (bytes_read > 0).then(|| {
        let secs = elapsed.as_secs_f64().max(1e-6);
        // Truncation is fine: this is a rough figure for the UI.
        (bytes_read as f64 / secs) as u64
    });
    Ok(SpeedReport {
        latency_ms,
        file,
        bytes_read,
        elapsed,
        bytes_per_sec,
    })
}

impl SourceManager {
    /// The file system of the source, with its password from the secret
    /// store. Does not connect yet: errors of the address or the password
    /// show up on the first call.
    ///
    /// SMB needs libsmb2 at run time (ADR-0033); without it this fails with
    /// [`ErrorKind::Unsupported`](pa_vfs::ErrorKind::Unsupported).
    pub fn connect(&self, id: i64) -> Result<Box<dyn Vfs>> {
        let stored = self.stored(id)?;
        let password = self.password_of(&stored)?;
        let masked = password.clone();
        backend(&stored, password).map_err(|e| source_error(id, &e, masked.as_ref()))
    }

    /// Lists the root of the source and records whether it is online and
    /// the error, if any. A source that cannot be reached is not an error
    /// of this call: see [`SourceStatus::online`] and
    /// [`SourceStatus::last_error`].
    pub fn check(&self, id: i64) -> Result<SourceStatus> {
        let stored = self.stored(id)?;
        let password = self.password_of(&stored)?;
        let masked = password.clone();
        let result =
            backend(&stored, password).and_then(|vfs| vfs.list(&VfsPath::root()).map(|_| ()));
        let error = result
            .err()
            .map(|e| scrub(&e.to_string(), masked.as_ref()));
        self.record_check(id, error.as_deref())?;
        Ok(self.get(id)?.status)
    }

    /// Measures the latency of a metadata request and the sequential read
    /// speed of the largest file found near the root, within `limits`. The
    /// result is saved in the status; a failure is saved as the last error
    /// and returned as [`Error::Source`].
    pub fn speed_test(&self, id: i64, limits: SpeedTestOptions) -> Result<SpeedReport> {
        let stored = self.stored(id)?;
        let password = self.password_of(&stored)?;
        let masked = password.clone();
        let result = backend(&stored, password).and_then(|vfs| measure(vfs.as_ref(), &limits));
        match result {
            Ok(report) => {
                self.record_speed(
                    id,
                    &SpeedSample {
                        bytes_per_sec: report.bytes_per_sec,
                        latency_ms: report.latency_ms,
                        measured_at_ms: now_ms(),
                    },
                )?;
                Ok(report)
            }
            Err(e) => {
                let err = source_error(id, &e, masked.as_ref());
                if let Error::Source { kind, message, .. } = &err {
                    self.record_check(id, Some(&format!("{kind}: {message}")))?;
                }
                Err(err)
            }
        }
    }
}
