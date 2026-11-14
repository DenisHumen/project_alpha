// SPDX-License-Identifier: MPL-2.0
//! Safe wrapper over one libsmb2 context (one TCP connection to one share).

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_int};
use std::fmt;
use std::ptr::NonNull;

use crate::error::{Error, Op};
use crate::library::Smb2Lib;
use crate::sys;

/// SMB dialect to negotiate (`smb2_set_version`).
///
/// libsmb2 implements SMB 2.0.2 to 3.1.1 only: there is no SMB1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Version {
    /// Let the server pick the highest common dialect (default).
    #[default]
    Any,
    /// Any SMB 2.x dialect.
    Any2,
    /// Any SMB 3.x dialect (needed for encryption).
    Any3,
    /// SMB 2.0.2.
    V2_02,
    /// SMB 2.1.
    V2_10,
    /// SMB 3.0.
    V3_00,
    /// SMB 3.0.2.
    V3_02,
    /// SMB 3.1.1.
    V3_11,
}

impl Version {
    fn raw(self) -> c_int {
        match self {
            Self::Any => 0,
            Self::Any2 => 2,
            Self::Any3 => 3,
            Self::V2_02 => 0x0202,
            Self::V2_10 => 0x0210,
            Self::V3_00 => 0x0300,
            Self::V3_02 => 0x0302,
            Self::V3_11 => 0x0311,
        }
    }
}

/// Message signing (`smb2_set_security_mode`, `smb2_set_sign`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Signing {
    /// Sign when the server requires it (default).
    #[default]
    Auto,
    /// Require signing; fail if the server cannot sign.
    Required,
}

/// SMB3 encryption (`smb2_set_seal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Encryption {
    /// Advertise support and encrypt when the server or share requires it
    /// (libsmb2's default; works with shares that mandate encryption).
    #[default]
    Auto,
    /// Require SMB3 encryption for everything; fail if not negotiated.
    Required,
    /// Never encrypt (fails on shares that mandate encryption).
    Off,
}

/// What `stat`, `fstat` and directory listings report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stat {
    /// File, directory or link.
    pub kind: FileType,
    /// Size in bytes.
    pub size: u64,
    /// Last write time, Unix seconds.
    pub mtime_sec: u64,
    /// Nanoseconds part of the last write time.
    pub mtime_nsec: u64,
}

/// Entry type as libsmb2 reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FileType {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A reparse point (symlink) the server did not follow.
    Link,
    /// A type value this wrapper does not know.
    Other,
}

impl From<sys::smb2_stat_64> for Stat {
    fn from(st: sys::smb2_stat_64) -> Self {
        Self {
            kind: match st.smb2_type {
                sys::SMB2_TYPE_FILE => FileType::File,
                sys::SMB2_TYPE_DIRECTORY => FileType::Dir,
                sys::SMB2_TYPE_LINK => FileType::Link,
                _ => FileType::Other,
            },
            size: st.smb2_size,
            mtime_sec: st.smb2_mtime,
            mtime_nsec: st.smb2_mtime_nsec,
        }
    }
}

/// One directory entry (without `.` and `..`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DirEntry {
    /// Entry name, UTF-8.
    pub name: String,
    /// Its metadata, from the same directory query.
    pub stat: Stat,
}

/// An open file on a [`Context`]. Only valid with the context that opened it;
/// using it with another context, or after `close`, fails with an error
/// instead of touching a dangling handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId {
    context: u64,
    id: u64,
}

/// One libsmb2 context: a connection to one share with its own session.
///
/// libsmb2 contexts are not thread safe, so `Context` is `Send` but not
/// `Sync`, and every call takes `&mut self`. Run several contexts for
/// parallel requests (one per thread, or a pool). All calls are blocking
/// and bounded by [`set_timeout`](Self::set_timeout).
///
/// Paths are relative to the share root, `/`-separated, without a leading
/// slash; `""` is the root.
pub struct Context {
    lib: &'static Smb2Lib,
    ptr: NonNull<sys::smb2_context>,
    files: HashMap<u64, NonNull<sys::smb2fh>>,
    next_file: u64,
    serial: u64,
    connected: bool,
}

// SAFETY: a libsmb2 context is plain heap state plus a socket, with no
// thread affinity; `Context` only hands it out through `&mut self`, so it is
// used by one thread at a time.
unsafe impl Send for Context {}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("connected", &self.connected)
            .field("open_files", &self.files.len())
            .finish_non_exhaustive()
    }
}

static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn cstring(op: Op, s: &str) -> Result<CString, Error> {
    CString::new(s).map_err(|_| Error::new(op, 0, 0, "string contains a NUL byte".into()))
}

impl Context {
    /// Creates a disconnected context with NTLMSSP authentication.
    pub fn new(lib: &'static Smb2Lib) -> Result<Self, Error> {
        // SAFETY: no preconditions.
        let ptr = unsafe { (lib.functions.init_context)() };
        let ptr = NonNull::new(ptr)
            .ok_or_else(|| Error::new(Op::Init, 0, 0, "smb2_init_context failed".into()))?;
        let ctx = Self {
            lib,
            ptr,
            files: HashMap::new(),
            next_file: 1,
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            connected: false,
        };
        // SAFETY: valid context.
        unsafe { (lib.functions.set_authentication)(ctx.raw(), sys::SMB2_SEC_NTLMSSP) };
        Ok(ctx)
    }

    fn raw(&self) -> *mut sys::smb2_context {
        self.ptr.as_ptr()
    }

    fn f(&self) -> &sys::Functions {
        &self.lib.functions
    }

    /// Per-request timeout in whole seconds (0 = libsmb2's default, none).
    pub fn set_timeout(&mut self, seconds: u32) {
        let s = c_int::try_from(seconds).unwrap_or(c_int::MAX);
        // SAFETY: valid context.
        unsafe { (self.f().set_timeout)(self.raw(), s) };
    }

    /// Dialect to negotiate; call before [`connect`](Self::connect).
    pub fn set_version(&mut self, version: Version) {
        // SAFETY: valid context; the value is a member of the C enum.
        unsafe { (self.f().set_version)(self.raw(), version.raw()) };
    }

    /// Signing policy; call before [`connect`](Self::connect).
    pub fn set_signing(&mut self, signing: Signing) {
        let (mode, sign) = match signing {
            Signing::Auto => (sys::SMB2_NEGOTIATE_SIGNING_ENABLED, 0),
            Signing::Required => (
                sys::SMB2_NEGOTIATE_SIGNING_ENABLED | sys::SMB2_NEGOTIATE_SIGNING_REQUIRED,
                1,
            ),
        };
        // SAFETY: valid context.
        unsafe {
            (self.f().set_security_mode)(self.raw(), mode);
            (self.f().set_sign)(self.raw(), sign);
        }
    }

    /// Encryption policy; call before [`connect`](Self::connect).
    /// [`Encryption::Auto`] leaves libsmb2's default untouched.
    pub fn set_encryption(&mut self, encryption: Encryption) {
        let seal = match encryption {
            Encryption::Auto => return,
            Encryption::Required => 1,
            Encryption::Off => 0,
        };
        // SAFETY: valid context.
        unsafe { (self.f().set_seal)(self.raw(), seal) };
    }

    /// NTLM credentials. An empty user and password log on as guest or
    /// anonymously, as the server allows. libsmb2 keeps its own copy of the
    /// password for the session; ours is wiped right after the call.
    pub fn set_credentials(
        &mut self,
        user: &str,
        password: &str,
        domain: Option<&str>,
    ) -> Result<(), Error> {
        let user_c = cstring(Op::Init, user)?;
        let mut pass = password.as_bytes().to_vec();
        if pass.contains(&0) {
            wipe(&mut pass);
            return Err(Error::new(Op::Init, 0, 0, "password contains a NUL byte".into()));
        }
        pass.push(0);
        // SAFETY: valid context and NUL-terminated strings; libsmb2 copies them.
        unsafe {
            (self.f().set_user)(self.raw(), user_c.as_ptr());
            (self.f().set_password)(self.raw(), pass.as_ptr().cast());
        }
        wipe(&mut pass);
        if let Some(domain) = domain {
            let d = cstring(Op::Init, domain)?;
            // SAFETY: as above.
            unsafe { (self.f().set_domain)(self.raw(), d.as_ptr()) };
        }
        Ok(())
    }

    /// Workstation name sent in the NTLM exchange.
    pub fn set_workstation(&mut self, name: &str) -> Result<(), Error> {
        let n = cstring(Op::Init, name)?;
        // SAFETY: valid context, NUL-terminated string copied by libsmb2.
        unsafe { (self.f().set_workstation)(self.raw(), n.as_ptr()) };
        Ok(())
    }

    /// Connects to `share` on `server` (`host` or `host:port`, IPv6 as
    /// `[addr]:port`), logging on as the user given to
    /// [`set_credentials`](Self::set_credentials).
    pub fn connect(&mut self, server: &str, share: &str, user: Option<&str>) -> Result<(), Error> {
        let s = cstring(Op::Connect, server)?;
        let sh = cstring(Op::Connect, share)?;
        let u = user.map(|u| cstring(Op::Connect, u)).transpose()?;
        self.clear_error();
        // SAFETY: valid context and strings.
        let rc = unsafe {
            (self.f().connect_share)(
                self.raw(),
                s.as_ptr(),
                sh.as_ptr(),
                u.as_ref().map_or(std::ptr::null(), |u| u.as_ptr()),
            )
        };
        if rc < 0 {
            return Err(self.error(Op::Connect, rc));
        }
        self.connected = true;
        Ok(())
    }

    /// `true` after a successful [`connect`](Self::connect).
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Negotiated dialect (`0x0202` … `0x0311`), 0 before connecting.
    #[must_use]
    pub fn dialect(&self) -> u16 {
        // SAFETY: valid context.
        unsafe { (self.f().get_dialect)(self.raw()) }
    }

    /// Largest read the server accepts in one request.
    #[must_use]
    pub fn max_read_size(&self) -> u32 {
        // SAFETY: valid context.
        unsafe { (self.f().get_max_read_size)(self.raw()) }
    }

    /// Metadata of `path`.
    pub fn stat(&mut self, path: &str) -> Result<Stat, Error> {
        let p = cstring(Op::Stat, path)?;
        let mut st = sys::smb2_stat_64::default();
        self.clear_error();
        // SAFETY: valid context, path and out-pointer.
        let rc = unsafe { (self.f().stat)(self.raw(), p.as_ptr(), &mut st) };
        if rc < 0 {
            return Err(self.error(Op::Stat, rc));
        }
        Ok(st.into())
    }

    /// Children of the directory `path`, without `.` and `..`. Names that
    /// are not valid UTF-8 are skipped.
    pub fn read_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, Error> {
        let p = cstring(Op::ListDir, path)?;
        self.clear_error();
        // SAFETY: valid context and path.
        let dir = unsafe { (self.f().opendir)(self.raw(), p.as_ptr()) };
        let Some(dir) = NonNull::new(dir) else {
            return Err(self.error(Op::ListDir, 0));
        };
        let mut out = Vec::new();
        loop {
            // SAFETY: `dir` is open on this context.
            let ent = unsafe { (self.f().readdir)(self.raw(), dir.as_ptr()) };
            if ent.is_null() {
                break;
            }
            // SAFETY: libsmb2 returns a valid entry with a NUL-terminated
            // name, alive until the next readdir; both are copied here.
            let (name, st) = unsafe { (CStr::from_ptr((*ent).name), (*ent).st) };
            let Ok(name) = name.to_str() else { continue };
            if name == "." || name == ".." {
                continue;
            }
            out.push(DirEntry {
                name: name.to_owned(),
                stat: st.into(),
            });
        }
        // SAFETY: `dir` is open on this context and not used afterwards.
        unsafe { (self.f().closedir)(self.raw(), dir.as_ptr()) };
        Ok(out)
    }

    /// Opens `path` read-only. A directory fails with
    /// [`ErrorKind::IsADirectory`](crate::ErrorKind::IsADirectory).
    pub fn open(&mut self, path: &str) -> Result<FileId, Error> {
        let p = cstring(Op::Open, path)?;
        self.clear_error();
        // SAFETY: valid context and path.
        let fh = unsafe { (self.f().open)(self.raw(), p.as_ptr(), sys::O_RDONLY) };
        let Some(fh) = NonNull::new(fh) else {
            return Err(self.error(Op::Open, 0));
        };
        let id = self.next_file;
        self.next_file += 1;
        self.files.insert(id, fh);
        Ok(FileId {
            context: self.serial,
            id,
        })
    }

    fn handle(&self, op: Op, file: FileId) -> Result<NonNull<sys::smb2fh>, Error> {
        if file.context != self.serial {
            return Err(Error::new(op, 0, 0, "file handle belongs to another context".into()));
        }
        self.files
            .get(&file.id)
            .copied()
            .ok_or_else(|| Error::new(op, 0, 0, "file handle is closed".into()))
    }

    /// Metadata of an open file.
    pub fn fstat(&mut self, file: FileId) -> Result<Stat, Error> {
        let fh = self.handle(Op::Fstat, file)?;
        let mut st = sys::smb2_stat_64::default();
        self.clear_error();
        // SAFETY: open handle of this context, valid out-pointer.
        let rc = unsafe { (self.f().fstat)(self.raw(), fh.as_ptr(), &mut st) };
        if rc < 0 {
            return Err(self.error(Op::Fstat, rc));
        }
        Ok(st.into())
    }

    /// Reads up to `buf.len()` bytes at `offset` (at most
    /// [`max_read_size`](Self::max_read_size) per call). Returns 0 at the
    /// end of the file.
    pub fn pread(&mut self, file: FileId, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let fh = self.handle(Op::Read, file)?;
        if buf.is_empty() {
            return Ok(0);
        }
        let max = match self.max_read_size() {
            0 => 64 * 1024,
            n => n,
        };
        let count = u32::try_from(buf.len()).unwrap_or(u32::MAX).min(max);
        self.clear_error();
        // SAFETY: open handle, `buf` has at least `count` writable bytes.
        let rc = unsafe { (self.f().pread)(self.raw(), fh.as_ptr(), buf.as_mut_ptr(), count, offset) };
        if rc < 0 {
            return Err(self.error(Op::Read, rc));
        }
        Ok(usize::try_from(rc).unwrap_or(0).min(buf.len()))
    }

    /// Closes an open file. The id is invalid afterwards.
    pub fn close(&mut self, file: FileId) -> Result<(), Error> {
        let fh = self.handle(Op::Close, file)?;
        self.files.remove(&file.id);
        self.clear_error();
        // SAFETY: open handle of this context, removed from the map first.
        let rc = unsafe { (self.f().close)(self.raw(), fh.as_ptr()) };
        if rc < 0 {
            return Err(self.error(Op::Close, rc));
        }
        Ok(())
    }

    /// Number of files currently open on this context.
    #[must_use]
    pub fn open_files(&self) -> usize {
        self.files.len()
    }

    fn clear_error(&mut self) {
        // SAFETY: valid context; an empty format string with no arguments.
        unsafe { (self.f().set_error)(self.raw(), c"".as_ptr()) };
    }

    fn error(&self, op: Op, rc: c_int) -> Error {
        // SAFETY: valid context; libsmb2 returns its own NUL-terminated buffer.
        let (msg, nt) = unsafe {
            let p = (self.f().get_error)(self.raw());
            let msg = if p.is_null() {
                String::new()
            } else {
                CStr::from_ptr(p).to_string_lossy().trim().to_owned()
            };
            (msg, (self.f().get_nterror)(self.raw()))
        };
        let msg = if msg.is_empty() {
            format!("libsmb2 returned {rc}")
        } else {
            msg
        };
        Error::new(op, rc.saturating_neg().max(0), nt as u32, msg)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        let raw = self.raw();
        let f = *self.f();
        for (_, fh) in self.files.drain() {
            // SAFETY: open handles of this context, each closed once.
            unsafe { (f.close)(raw, fh.as_ptr()) };
        }
        // SAFETY: valid context, destroyed once; destroy also disconnects.
        unsafe {
            if self.connected {
                (f.disconnect_share)(raw);
            }
            (f.destroy_context)(raw);
        }
    }
}

fn wipe(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        // SAFETY: valid exclusive reference; volatile so it is not elided.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
}
