// SPDX-License-Identifier: MPL-2.0
//! WebDAV backend of [`pa_vfs`] (NET-004).
//!
//! Plays media straight from Nextcloud, ownCloud, a NAS (Synology, QNAP,
//! TrueNAS), Apache `mod_dav`, nginx with `dav-ext`, `rclone serve webdav`
//! and any other RFC 4918 server. [`WebDavFs`] implements [`pa_vfs::Vfs`]
//! and passes the shared contract suite (`pa_vfs::vfs_contract_tests!`)
//! against an in-process server in every test run and against real servers
//! in Docker (`tests/servers/`).
//!
//! ```no_run
//! use pa_vfs::{ByteSource, Vfs, VfsPath};
//! use pa_vfs_webdav::{Credentials, WebDavConfig, WebDavFs};
//!
//! let fs = WebDavFs::new(
//!     WebDavConfig::new("https://cloud.example/remote.php/dav/files/anna/")
//!         .with_credentials(Credentials::new("anna", "app-password")),
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
//! Self-signed certificates: see [`TlsTrust`] and [`fetch_fingerprint`].
//! The design choices (blocking reqwest, own redirect and Digest handling,
//! TOFU pinning) are recorded in ADR-NNNN.

mod auth;
mod client;
mod config;
mod href;
mod propfind;
mod source;
mod tls;

pub use client::WebDavFs;
pub use config::{Credentials, DEFAULT_READ_AHEAD, WebDavConfig};
pub use pa_discovery::Fingerprint;
pub use tls::{TlsTrust, UntrustedCertificate, fetch_fingerprint, is_untrusted_certificate};
