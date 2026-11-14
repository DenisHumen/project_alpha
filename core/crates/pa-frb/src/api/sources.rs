// SPDX-License-Identifier: MPL-2.0
//! The source manager (`pa-sources`, NET-011) for Dart.
//!
//! [`CoreSources`] keeps local, SMB and WebDAV sources in the library
//! database and their passwords in the OS secret store (`pa-secrets`).
//! Passwords go from Dart to Rust only; no call returns one.
//!
//! On Android and iOS the platform keystore callback is not wired through
//! this facade yet (CORE-015): there passwords go to the encrypted fallback
//! file in `data_dir`. Desktop uses the OS keychain or the same fallback.
//! The store is shared with [`CoreLibrary`](super::library::CoreLibrary),
//! which reads the passwords to scan and play network sources (LIB-018).
//!
//! Every method blocks (SQLite, keychain, network), so none is
//! `#[frb(sync)]`.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use flutter_rust_bridge::frb;
use pa_secrets::SecretString;
use pa_sources as s;

/// Error of a source manager call; Dart receives it as an exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcesError {
    /// `not_found`, `invalid`, `secrets`, `unreachable` or `other`.
    pub code: String,
    /// Human-readable message, secrets masked.
    pub message: String,
}

impl From<s::Error> for SourcesError {
    fn from(e: s::Error) -> Self {
        let code = match &e {
            s::Error::NotFound(_) => "not_found",
            s::Error::Invalid(_) => "invalid",
            s::Error::Secrets(_) => "secrets",
            s::Error::Source { .. } => "unreachable",
            _ => "other",
        };
        Self {
            code: code.into(),
            message: e.to_string(),
        }
    }
}

type SourcesResult<T> = std::result::Result<T, SourcesError>;

fn invalid(message: String) -> SourcesError {
    SourcesError {
        code: "invalid".into(),
        message,
    }
}

/// Backend options; see `pa_sources::SourceOptions`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoreSourceOptions {
    pub smb_domain: Option<String>,
    /// `any`, `smb2` or `smb3`.
    pub smb_version: String,
    pub smb_signing_required: bool,
    /// `auto`, `required` or `off`.
    pub smb_encryption: String,
    /// WebDAV certificate pin, `sha256:<64 hex>`.
    pub tls_pin: Option<String>,
    pub timeout_secs: Option<u32>,
    pub read_ahead_bytes: Option<u64>,
}

impl From<s::SourceOptions> for CoreSourceOptions {
    fn from(o: s::SourceOptions) -> Self {
        Self {
            smb_domain: o.smb_domain,
            smb_version: match o.smb_version {
                s::SmbVersion::Any => "any",
                s::SmbVersion::Smb2 => "smb2",
                s::SmbVersion::Smb3 => "smb3",
            }
            .into(),
            smb_signing_required: o.smb_signing_required,
            smb_encryption: match o.smb_encryption {
                s::SmbEncryption::Auto => "auto",
                s::SmbEncryption::Required => "required",
                s::SmbEncryption::Off => "off",
            }
            .into(),
            tls_pin: o.tls_pin,
            timeout_secs: o.timeout_secs,
            read_ahead_bytes: o.read_ahead_bytes,
        }
    }
}

impl TryFrom<CoreSourceOptions> for s::SourceOptions {
    type Error = SourcesError;

    fn try_from(o: CoreSourceOptions) -> SourcesResult<Self> {
        Ok(Self {
            smb_domain: o.smb_domain.filter(|d| !d.is_empty()),
            smb_version: match o.smb_version.as_str() {
                "" | "any" => s::SmbVersion::Any,
                "smb2" => s::SmbVersion::Smb2,
                "smb3" => s::SmbVersion::Smb3,
                other => return Err(invalid(format!("SMB version {other:?}"))),
            },
            smb_signing_required: o.smb_signing_required,
            smb_encryption: match o.smb_encryption.as_str() {
                "" | "auto" => s::SmbEncryption::Auto,
                "required" => s::SmbEncryption::Required,
                "off" => s::SmbEncryption::Off,
                other => return Err(invalid(format!("SMB encryption {other:?}"))),
            },
            tls_pin: o.tls_pin.filter(|p| !p.is_empty()),
            timeout_secs: o.timeout_secs,
            read_ahead_bytes: o.read_ahead_bytes,
        })
    }
}

/// Device-local status of a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSourceStatus {
    pub online: Option<bool>,
    pub checked_at_ms: Option<i64>,
    pub last_scan_started_at_ms: Option<i64>,
    pub last_scan_finished_at_ms: Option<i64>,
    pub scanning: bool,
    pub last_error: Option<String>,
    pub last_error_at_ms: Option<i64>,
    pub speed_bytes_per_sec: Option<u64>,
    pub speed_latency_ms: Option<u64>,
    pub speed_measured_at_ms: Option<i64>,
}

impl From<s::SourceStatus> for CoreSourceStatus {
    fn from(st: s::SourceStatus) -> Self {
        Self {
            scanning: st.is_scanning(),
            online: st.online,
            checked_at_ms: st.checked_at_ms,
            last_scan_started_at_ms: st.last_scan_started_at_ms,
            last_scan_finished_at_ms: st.last_scan_finished_at_ms,
            last_error: st.last_error,
            last_error_at_ms: st.last_error_at_ms,
            speed_bytes_per_sec: st.speed.and_then(|x| x.bytes_per_sec),
            speed_latency_ms: st.speed.map(|x| x.latency_ms),
            speed_measured_at_ms: st.speed.map(|x| x.measured_at_ms),
        }
    }
}

/// A stored source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreManagedSource {
    pub id: i64,
    /// `local`, `smb` or `webdav`.
    pub kind: String,
    pub name: String,
    /// Root URI without credentials.
    pub uri: String,
    pub username: Option<String>,
    pub has_password: bool,
    pub options: CoreSourceOptions,
    pub scan_into_library: bool,
    pub auto_scan: bool,
    pub enabled: bool,
    pub status: CoreSourceStatus,
}

impl From<s::Source> for CoreManagedSource {
    fn from(x: s::Source) -> Self {
        Self {
            id: x.id,
            kind: x.kind.as_str().into(),
            name: x.name,
            uri: x.uri,
            username: x.username,
            has_password: x.has_password,
            options: x.options.into(),
            scan_into_library: x.scan_into_library,
            auto_scan: x.auto_scan,
            enabled: x.enabled,
            status: x.status.into(),
        }
    }
}

/// Settings of a new or edited source. `Debug` hides the password.
#[derive(Clone, PartialEq, Eq)]
pub struct CoreSourceInput {
    /// `local`, `smb` or `webdav`; ignored on update.
    pub kind: String,
    pub name: String,
    /// Folder path or URI without credentials.
    pub address: String,
    pub username: Option<String>,
    /// On create: the password. On update: `None` keeps the stored one,
    /// an empty string removes it.
    pub password: Option<String>,
    pub options: CoreSourceOptions,
    pub scan_into_library: bool,
    pub auto_scan: bool,
    pub enabled: bool,
}

impl fmt::Debug for CoreSourceInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoreSourceInput")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("address", &self.address)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .finish_non_exhaustive()
    }
}

/// Result of a speed test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSpeedReport {
    pub latency_ms: u64,
    /// Path of the file that was read, inside the source.
    pub file: Option<String>,
    pub bytes_read: u64,
    pub elapsed_ms: u64,
    pub bytes_per_sec: Option<u64>,
}

/// The source manager of one library database.
#[frb(opaque)]
pub struct CoreSources {
    inner: s::SourceManager,
    backend: String,
    fallback_reason: Option<String>,
}

impl CoreSources {
    /// Opens the library database at `db_path` and the secret store
    /// (keychain, or the encrypted fallback file in `data_dir`).
    pub fn open(db_path: String, data_dir: String) -> SourcesResult<CoreSources> {
        let opened =
            crate::shared::secret_store(std::path::Path::new(&data_dir)).map_err(s::Error::from)?;
        Ok(Self {
            backend: opened.backend().as_str().into(),
            fallback_reason: opened.fallback_reason,
            inner: s::SourceManager::open(db_path, Arc::clone(&opened.store))?,
        })
    }

    /// Where passwords are kept: `keychain`, `secret-service`,
    /// `credential-manager`, `encrypted-file`…
    #[frb(sync)]
    pub fn secret_backend(&self) -> String {
        self.backend.clone()
    }

    /// Why the encrypted fallback file is used (the app warns the user).
    #[frb(sync)]
    pub fn secret_fallback_reason(&self) -> Option<String> {
        self.fallback_reason.clone()
    }

    /// All sources in display order.
    pub fn list(&self) -> SourcesResult<Vec<CoreManagedSource>> {
        Ok(self.inner.list()?.into_iter().map(Into::into).collect())
    }

    /// One source.
    pub fn get(&self, id: i64) -> SourcesResult<CoreManagedSource> {
        Ok(self.inner.get(id)?.into())
    }

    /// Adds a source.
    pub fn create(&self, input: CoreSourceInput) -> SourcesResult<CoreManagedSource> {
        let kind: s::SourceKind = input.kind.parse()?;
        let new = s::NewSource {
            kind,
            name: input.name,
            address: input.address,
            username: input.username,
            password: input.password.map(SecretString::new),
            options: input.options.try_into()?,
            scan_into_library: input.scan_into_library,
            auto_scan: input.auto_scan,
            enabled: input.enabled,
        };
        Ok(self.inner.create(new)?.into())
    }

    /// Replaces the settings of source `id` (the kind cannot change).
    pub fn update(&self, id: i64, input: CoreSourceInput) -> SourcesResult<CoreManagedSource> {
        let password = match input.password {
            None => s::PasswordChange::Keep,
            Some(p) if p.is_empty() => s::PasswordChange::Clear,
            Some(p) => s::PasswordChange::Set(SecretString::new(p)),
        };
        let update = s::SourceUpdate {
            name: Some(input.name),
            address: Some(input.address),
            username: Some(input.username),
            password,
            options: Some(input.options.try_into()?),
            scan_into_library: Some(input.scan_into_library),
            auto_scan: Some(input.auto_scan),
            enabled: Some(input.enabled),
        };
        Ok(self.inner.update(id, update)?.into())
    }

    /// Deletes a source and its password.
    pub fn delete(&self, id: i64) -> SourcesResult<()> {
        Ok(self.inner.delete(id)?)
    }

    /// Checks that the source is reachable; the result is in the status.
    pub fn check(&self, id: i64) -> SourcesResult<CoreSourceStatus> {
        Ok(self.inner.check(id)?.into())
    }

    /// Measures latency and read speed for at most `max_seconds`.
    pub fn speed_test(&self, id: i64, max_seconds: u32) -> SourcesResult<CoreSpeedReport> {
        let limits = s::SpeedTestOptions {
            max_duration: Duration::from_secs(max_seconds.clamp(1, 60).into()),
            ..Default::default()
        };
        let r = self.inner.speed_test(id, limits)?;
        Ok(CoreSpeedReport {
            latency_ms: r.latency_ms,
            file: r.file.map(|p| p.as_str().to_owned()),
            bytes_read: r.bytes_read,
            elapsed_ms: u64::try_from(r.elapsed.as_millis()).unwrap_or(u64::MAX),
            bytes_per_sec: r.bytes_per_sec,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(kind: &str, address: &str, password: Option<&str>) -> CoreSourceInput {
        CoreSourceInput {
            kind: kind.into(),
            name: "Src".into(),
            address: address.into(),
            username: password.map(|_| "anna".into()),
            password: password.map(Into::into),
            options: CoreSourceOptions::default(),
            scan_into_library: true,
            auto_scan: true,
            enabled: true,
        }
    }

    #[test]
    fn crud_through_the_facade() {
        let dir = tempfile::tempdir().unwrap();
        let sources = CoreSources {
            inner: s::SourceManager::open(
                dir.path().join("library.db"),
                Arc::new(pa_secrets::MemoryStore::new()),
            )
            .unwrap(),
            backend: "memory".into(),
            fallback_reason: None,
        };
        let media = dir.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let local = sources
            .create(input("local", media.to_str().unwrap(), None))
            .unwrap();
        assert_eq!(local.kind, "local");
        assert_eq!(sources.check(local.id).unwrap().online, Some(true));

        let mut smb = input("smb", "smb://nas/media", Some("hunter2-frb"));
        smb.options.smb_version = "smb3".into();
        let created = sources.create(smb.clone()).unwrap();
        assert!(created.has_password);
        assert_eq!(created.options.smb_version, "smb3");
        assert!(!format!("{smb:?}").contains("hunter2-frb"));

        smb.password = Some(String::new());
        assert!(!sources.update(created.id, smb).unwrap().has_password);
        assert_eq!(sources.list().unwrap().len(), 2);
        sources.delete(created.id).unwrap();
        let err = sources.get(created.id).unwrap_err();
        assert_eq!(err.code, "not_found");
        let err = sources.create(input("nfs", "nfs://x", None)).unwrap_err();
        assert_eq!(err.code, "invalid");
    }
}
