// SPDX-License-Identifier: MPL-2.0
//! Raw C ABI of libsmb2 6.x (`include/smb2/libsmb2.h`): the types and the
//! function table this crate loads. Only what [`Context`](crate::Context)
//! uses; the list is mirrored in `tools/libsmb2/lib/smb2.sh`
//! (`SMB2_REQUIRED_SYMBOLS`), and the licence gate checks every artifact
//! exports it.

#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int};
use std::marker::{PhantomData, PhantomPinned};

macro_rules! opaque {
    ($($name:ident),+) => {$(
        /// Opaque libsmb2 handle.
        #[repr(C)]
        pub struct $name {
            _data: [u8; 0],
            _marker: PhantomData<(*mut u8, PhantomPinned)>,
        }
    )+};
}

opaque!(smb2_context, smb2dir, smb2fh);

/// `SMB2_TYPE_FILE`.
pub const SMB2_TYPE_FILE: u32 = 0;
/// `SMB2_TYPE_DIRECTORY`.
pub const SMB2_TYPE_DIRECTORY: u32 = 1;
/// `SMB2_TYPE_LINK`: a reparse point the server did not follow.
pub const SMB2_TYPE_LINK: u32 = 2;

/// `SMB2_NEGOTIATE_SIGNING_ENABLED`.
pub const SMB2_NEGOTIATE_SIGNING_ENABLED: u16 = 0x0001;
/// `SMB2_NEGOTIATE_SIGNING_REQUIRED`.
pub const SMB2_NEGOTIATE_SIGNING_REQUIRED: u16 = 0x0002;

/// `SMB2_SEC_NTLMSSP` (`enum smb2_sec`): libsmb2 is built without Kerberos.
pub const SMB2_SEC_NTLMSSP: c_int = 1;

/// `O_RDONLY`, the same (0) on every supported OS.
pub const O_RDONLY: c_int = 0;

/// `struct smb2_stat_64`. Times are Unix seconds plus nanoseconds.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct smb2_stat_64 {
    pub smb2_type: u32,
    pub smb2_nlink: u32,
    pub smb2_ino: u64,
    pub smb2_size: u64,
    pub smb2_atime: u64,
    pub smb2_atime_nsec: u64,
    pub smb2_mtime: u64,
    pub smb2_mtime_nsec: u64,
    pub smb2_ctime: u64,
    pub smb2_ctime_nsec: u64,
    pub smb2_btime: u64,
    pub smb2_btime_nsec: u64,
}

/// `struct smb2dirent`; valid until the next `smb2_readdir`/`smb2_closedir`.
#[repr(C)]
pub struct smb2dirent {
    pub name: *const c_char,
    pub st: smb2_stat_64,
}

type Ctx = *mut smb2_context;

/// Every libsmb2 function the wrapper calls, resolved by name at load time.
#[allow(missing_docs)]
#[derive(Clone, Copy)]
pub struct Functions {
    pub init_context: unsafe extern "C" fn() -> Ctx,
    pub destroy_context: unsafe extern "C" fn(Ctx),
    pub set_timeout: unsafe extern "C" fn(Ctx, c_int),
    pub set_version: unsafe extern "C" fn(Ctx, c_int),
    pub set_security_mode: unsafe extern "C" fn(Ctx, u16),
    pub set_seal: unsafe extern "C" fn(Ctx, c_int),
    pub set_sign: unsafe extern "C" fn(Ctx, c_int),
    pub set_authentication: unsafe extern "C" fn(Ctx, c_int),
    pub set_user: unsafe extern "C" fn(Ctx, *const c_char),
    pub set_password: unsafe extern "C" fn(Ctx, *const c_char),
    pub set_domain: unsafe extern "C" fn(Ctx, *const c_char),
    pub set_workstation: unsafe extern "C" fn(Ctx, *const c_char),
    pub connect_share:
        unsafe extern "C" fn(Ctx, *const c_char, *const c_char, *const c_char) -> c_int,
    pub disconnect_share: unsafe extern "C" fn(Ctx) -> c_int,
    pub get_error: unsafe extern "C" fn(Ctx) -> *const c_char,
    /// Variadic `printf`-style; called only with `""`, which also resets
    /// the NT status, so the status read after a call belongs to it.
    pub set_error: unsafe extern "C" fn(Ctx, *const c_char, ...),
    pub get_nterror: unsafe extern "C" fn(Ctx) -> c_int,
    pub get_dialect: unsafe extern "C" fn(Ctx) -> u16,
    pub get_max_read_size: unsafe extern "C" fn(Ctx) -> u32,
    pub opendir: unsafe extern "C" fn(Ctx, *const c_char) -> *mut smb2dir,
    pub readdir: unsafe extern "C" fn(Ctx, *mut smb2dir) -> *mut smb2dirent,
    pub closedir: unsafe extern "C" fn(Ctx, *mut smb2dir),
    pub open: unsafe extern "C" fn(Ctx, *const c_char, c_int) -> *mut smb2fh,
    pub close: unsafe extern "C" fn(Ctx, *mut smb2fh) -> c_int,
    pub pread: unsafe extern "C" fn(Ctx, *mut smb2fh, *mut u8, u32, u64) -> c_int,
    pub fstat: unsafe extern "C" fn(Ctx, *mut smb2fh, *mut smb2_stat_64) -> c_int,
    pub stat: unsafe extern "C" fn(Ctx, *const c_char, *mut smb2_stat_64) -> c_int,
}

/// Names of [`Functions`] fields in the library, in declaration order.
pub const SYMBOLS: &[&str] = &[
    "smb2_init_context",
    "smb2_destroy_context",
    "smb2_set_timeout",
    "smb2_set_version",
    "smb2_set_security_mode",
    "smb2_set_seal",
    "smb2_set_sign",
    "smb2_set_authentication",
    "smb2_set_user",
    "smb2_set_password",
    "smb2_set_domain",
    "smb2_set_workstation",
    "smb2_connect_share",
    "smb2_disconnect_share",
    "smb2_get_error",
    "smb2_set_error",
    "smb2_get_nterror",
    "smb2_get_dialect",
    "smb2_get_max_read_size",
    "smb2_opendir",
    "smb2_readdir",
    "smb2_closedir",
    "smb2_open",
    "smb2_close",
    "smb2_pread",
    "smb2_fstat",
    "smb2_stat",
];

impl Functions {
    /// Resolves every function from `lib`.
    ///
    /// # Safety
    ///
    /// `lib` must be libsmb2 6.x (the signatures above are not checked).
    pub(crate) unsafe fn load(lib: &libloading::Library) -> Result<Self, String> {
        macro_rules! sym {
            // SAFETY: the caller guarantees `lib` is libsmb2, whose function
            // has the C type of the field it is assigned to.
            ($name:literal) => {
                unsafe { get(lib, concat!($name, "\0")) }?
            };
        }
        Ok(Self {
            init_context: sym!("smb2_init_context"),
            destroy_context: sym!("smb2_destroy_context"),
            set_timeout: sym!("smb2_set_timeout"),
            set_version: sym!("smb2_set_version"),
            set_security_mode: sym!("smb2_set_security_mode"),
            set_seal: sym!("smb2_set_seal"),
            set_sign: sym!("smb2_set_sign"),
            set_authentication: sym!("smb2_set_authentication"),
            set_user: sym!("smb2_set_user"),
            set_password: sym!("smb2_set_password"),
            set_domain: sym!("smb2_set_domain"),
            set_workstation: sym!("smb2_set_workstation"),
            connect_share: sym!("smb2_connect_share"),
            disconnect_share: sym!("smb2_disconnect_share"),
            get_error: sym!("smb2_get_error"),
            set_error: sym!("smb2_set_error"),
            get_nterror: sym!("smb2_get_nterror"),
            get_dialect: sym!("smb2_get_dialect"),
            get_max_read_size: sym!("smb2_get_max_read_size"),
            opendir: sym!("smb2_opendir"),
            readdir: sym!("smb2_readdir"),
            closedir: sym!("smb2_closedir"),
            open: sym!("smb2_open"),
            close: sym!("smb2_close"),
            pread: sym!("smb2_pread"),
            fstat: sym!("smb2_fstat"),
            stat: sym!("smb2_stat"),
        })
    }
}

/// Resolves one symbol as a value of type `T` (a function pointer).
///
/// # Safety
///
/// The symbol must have type `T`.
unsafe fn get<T: Copy>(lib: &libloading::Library, name: &str) -> Result<T, String> {
    // SAFETY: forwarded to the caller.
    match unsafe { lib.get::<T>(name.as_bytes()) } {
        Ok(symbol) => Ok(*symbol),
        Err(e) => Err(format!("{}: {e}", name.trim_end_matches('\0'))),
    }
}
