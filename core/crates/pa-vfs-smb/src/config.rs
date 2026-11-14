// SPDX-License-Identifier: MPL-2.0
//! Connection settings of an SMB source.

use std::fmt;
use std::time::Duration;

use pa_smb2::{Encryption, Signing, Smb2Lib, Version};

/// Default read-ahead buffer per open file while it is read sequentially.
pub const DEFAULT_READ_AHEAD: usize = 64 * 1024 * 1024;
/// Default size of one read-ahead block (one or more SMB READ requests).
pub const DEFAULT_CHUNK_SIZE: usize = 1024 * 1024;
/// Default number of blocks fetched in parallel, each on its own connection.
pub const DEFAULT_PREFETCH_PARALLEL: usize = 4;
/// Default upper bound of pooled connections per source.
pub const DEFAULT_MAX_CONNECTIONS: usize = 6;
/// Default timeout of one SMB request.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// User name and password for NTLM logon.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub(crate) user: String,
    pub(crate) password: String,
    pub(crate) domain: Option<String>,
}

impl Credentials {
    /// A local or domain user without a domain prefix.
    pub fn new(user: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            password: password.into(),
            domain: None,
        }
    }

    /// Adds the NTLM domain (workgroup) of the user.
    #[must_use]
    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    /// The user name.
    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .field("domain", &self.domain)
            .finish()
    }
}

/// Where and how to connect: server, share, folder inside it, credentials,
/// protocol options and the read-ahead buffer.
#[derive(Clone)]
pub struct SmbConfig {
    pub(crate) server: String,
    pub(crate) share: String,
    pub(crate) root: String,
    pub(crate) credentials: Option<Credentials>,
    pub(crate) version: Version,
    pub(crate) signing: Signing,
    pub(crate) encryption: Encryption,
    pub(crate) allow_smb1: bool,
    pub(crate) timeout: Duration,
    pub(crate) max_connections: usize,
    pub(crate) read_ahead: usize,
    pub(crate) chunk_size: usize,
    pub(crate) prefetch_parallel: usize,
    pub(crate) lib: Option<&'static Smb2Lib>,
}

impl SmbConfig {
    /// `server` is `host`, `host:port` or `[ipv6]:port` (port 445 by
    /// default); `share` is the share name without slashes.
    pub fn new(server: impl Into<String>, share: impl Into<String>) -> Self {
        Self {
            server: server.into(),
            share: share.into(),
            root: String::new(),
            credentials: None,
            version: Version::Any,
            signing: Signing::Auto,
            encryption: Encryption::Auto,
            allow_smb1: false,
            timeout: DEFAULT_TIMEOUT,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            read_ahead: DEFAULT_READ_AHEAD,
            chunk_size: DEFAULT_CHUNK_SIZE,
            prefetch_parallel: DEFAULT_PREFETCH_PARALLEL,
            lib: None,
        }
    }

    /// Makes a folder inside the share the source root (`/` of the
    /// [`Vfs`](pa_vfs::Vfs)); `/`-separated, leading and trailing slashes
    /// are ignored.
    #[must_use]
    pub fn with_root(mut self, folder: &str) -> Self {
        self.root = folder
            .split('/')
            .filter(|c| !c.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        self
    }

    /// Logs on as this user. Without credentials the backend logs on
    /// anonymously, which most servers allow only for guest shares.
    #[must_use]
    pub fn with_credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Dialect to negotiate; default: the highest the server offers.
    #[must_use]
    pub fn with_version(mut self, version: Version) -> Self {
        self.version = version;
        self
    }

    /// Signing policy; default: sign when the server requires it.
    #[must_use]
    pub fn with_signing(mut self, signing: Signing) -> Self {
        self.signing = signing;
        self
    }

    /// SMB3 encryption policy; default: encrypt when the server or share
    /// requires it.
    #[must_use]
    pub fn with_encryption(mut self, encryption: Encryption) -> Self {
        self.encryption = encryption;
        self
    }

    /// Explicit opt-in to SMB1 (NET-003: "only by an explicit flag, with a
    /// warning"). libsmb2 does not implement SMB1, so with this flag
    /// [`SmbFs::new`](crate::SmbFs::new) fails with
    /// [`ErrorKind::Unsupported`](pa_vfs::ErrorKind::Unsupported) and a
    /// message telling the user to enable SMB2 on the server.
    #[must_use]
    pub fn with_smb1(mut self, allow: bool) -> Self {
        self.allow_smb1 = allow;
        self
    }

    /// Timeout of one request (rounded up to whole seconds).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Upper bound of pooled connections (at least 1; default 6).
    #[must_use]
    pub fn with_max_connections(mut self, n: usize) -> Self {
        self.max_connections = n.max(1);
        self
    }

    /// Read-ahead buffer per file read sequentially (default 64 MiB; 32–256
    /// MiB is the useful range for 4K remuxes over Wi-Fi). Below two blocks
    /// read-ahead is off and every read goes to the server.
    #[must_use]
    pub fn with_read_ahead(mut self, bytes: usize) -> Self {
        self.read_ahead = bytes;
        self
    }

    /// Size of one read-ahead block (default 1 MiB, at least 4 KiB).
    #[must_use]
    pub fn with_chunk_size(mut self, bytes: usize) -> Self {
        self.chunk_size = bytes.max(4096);
        self
    }

    /// Blocks fetched in parallel ahead of the reader (default 4, 0 turns
    /// background prefetch off).
    #[must_use]
    pub fn with_prefetch_parallel(mut self, n: usize) -> Self {
        self.prefetch_parallel = n;
        self
    }

    /// Uses this libsmb2 instead of [`Smb2Lib::shared`].
    #[must_use]
    pub fn with_library(mut self, lib: &'static Smb2Lib) -> Self {
        self.lib = Some(lib);
        self
    }

    /// Number of read-ahead blocks kept per file.
    pub(crate) fn capacity_chunks(&self) -> usize {
        self.read_ahead / self.chunk_size.max(1)
    }

    pub(crate) fn timeout_secs(&self) -> u32 {
        let s = self.timeout.as_secs() + u64::from(self.timeout.subsec_nanos() > 0);
        u32::try_from(s.max(1)).unwrap_or(u32::MAX)
    }
}

impl fmt::Debug for SmbConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmbConfig")
            .field("server", &self.server)
            .field("share", &self.share)
            .field("root", &self.root)
            .field("credentials", &self.credentials)
            .field("version", &self.version)
            .field("signing", &self.signing)
            .field("encryption", &self.encryption)
            .field("read_ahead", &self.read_ahead)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_password() {
        let c = SmbConfig::new("nas", "media")
            .with_credentials(Credentials::new("anna", "hunter2").with_domain("WG"));
        let s = format!("{c:?}");
        assert!(s.contains("anna") && !s.contains("hunter2"), "{s}");
    }

    #[test]
    fn root_and_limits_are_normalised() {
        let c = SmbConfig::new("nas", "media")
            .with_root("/Movies//4K/")
            .with_max_connections(0)
            .with_chunk_size(1)
            .with_timeout(Duration::from_millis(1500));
        assert_eq!(c.root, "Movies/4K");
        assert_eq!(c.max_connections, 1);
        assert_eq!(c.chunk_size, 4096);
        assert_eq!(c.timeout_secs(), 2);
        assert_eq!(SmbConfig::new("a", "b").capacity_chunks(), 64);
    }
}
