// SPDX-License-Identifier: MPL-2.0
//! SMB2/3 backend of [`pa_vfs`] over libsmb2 (NET-003).
//!
//! Plays media straight from Windows shares, Samba and NAS boxes
//! (Synology, QNAP, TrueNAS, routers): SMB 2.0.2 to 3.1.1, NTLMSSP logon,
//! signing and SMB3 encryption. [`SmbFs`] implements [`pa_vfs::Vfs`] and
//! passes the shared contract suite against Samba in Docker
//! (`tests/servers/`).
//!
//! libsmb2 (LGPL) is loaded at run time through [`pa_smb2`]; without it
//! [`SmbFs::new`] fails with [`ErrorKind::Unsupported`](pa_vfs::ErrorKind::Unsupported)
//! and the rest of the app keeps working. Design: ADR-NNNN.
//!
//! ```no_run
//! use pa_vfs::{ByteSource, Vfs, VfsPath};
//! use pa_vfs_smb::{Credentials, SmbConfig, SmbFs};
//!
//! let fs = SmbFs::new(
//!     SmbConfig::new("nas.local", "media")
//!         .with_credentials(Credentials::new("anna", "secret"))
//!         .with_read_ahead(128 * 1024 * 1024),
//! )?;
//! for entry in fs.list(&VfsPath::parse("/Movies")?)? {
//!     println!("{} {:?}", entry.name, entry.metadata.kind);
//! }
//! let movie = fs.open(&VfsPath::parse("/Movies/Dune (2021).mkv")?)?;
//! let mut header = [0u8; 4];
//! movie.read_exact_at(0, &mut header)?;
//! # Ok::<(), pa_vfs::VfsError>(())
//! ```
//!
//! Connections: a pool of libsmb2 contexts (up to
//! [`DEFAULT_MAX_CONNECTIONS`]), each serving one request at a time. A
//! dropped connection is replaced and the request retried once.
//!
//! Reading: see [`SmbSource`] — random reads go to the server as is;
//! sequential reads use a read-ahead buffer ([`DEFAULT_READ_AHEAD`],
//! configurable) filled by [`DEFAULT_PREFETCH_PARALLEL`] blocks in
//! parallel.

mod config;
mod fs;
mod pool;
mod prefetch;
mod source;

pub use config::{
    Credentials, DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CONNECTIONS, DEFAULT_PREFETCH_PARALLEL,
    DEFAULT_READ_AHEAD, DEFAULT_TIMEOUT, SmbConfig,
};
pub use fs::SmbFs;
pub use pa_smb2::{Encryption, Signing, Smb2Lib, Version};
pub use source::SmbSource;
