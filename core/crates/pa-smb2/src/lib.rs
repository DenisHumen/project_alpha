// SPDX-License-Identifier: MPL-2.0
//! Safe Rust wrapper over [libsmb2](https://github.com/sahlberg/libsmb2)
//! (NET-002).
//!
//! libsmb2 is LGPL-2.1+, so it is never linked into our binaries: it is
//! built as a shared library by `tools/libsmb2` (an embedded
//! `smb2.framework` on iOS and tvOS) and loaded at run time with
//! `libloading` ([`Smb2Lib`]), see ADR-NNNN. The workspace builds and
//! tests without it; tests that need it read `$PA_SMB2_LIB` and are skipped
//! when it is unset.
//!
//! [`Context`] is one connection to one share: SMB 2.0.2 to 3.1.1, NTLMSSP,
//! signing ([`Signing`]) and SMB3 encryption ([`Encryption`]), blocking
//! `stat`, directory listing, `open` and positional `pread`. The SMB backend
//! of `pa-vfs` (NET-003) pools several contexts for parallel reads.
//!
//! ```no_run
//! use pa_smb2::{Context, Encryption, Smb2Lib};
//!
//! let lib = Smb2Lib::shared()?;
//! let mut ctx = Context::new(lib)?;
//! ctx.set_timeout(30);
//! ctx.set_encryption(Encryption::Auto);
//! ctx.set_credentials("anna", "secret", None)?;
//! ctx.connect("nas.local", "media", Some("anna"))?;
//! for entry in ctx.read_dir("Movies")? {
//!     println!("{} {} bytes", entry.name, entry.stat.size);
//! }
//! let file = ctx.open("Movies/Dune (2021).mkv")?;
//! let mut header = [0u8; 4];
//! let n = ctx.pread(file, 0, &mut header)?;
//! ctx.close(file)?;
//! # let _ = n;
//! # Ok::<(), pa_smb2::Error>(())
//! ```

mod context;
mod error;
mod library;
pub mod sys;

pub use context::{Context, DirEntry, Encryption, FileId, FileType, Signing, Stat, Version};
pub use error::{Error, ErrorKind, Op};
pub use library::{LIB_ENV, Smb2Lib, bundled_paths, default_names};
