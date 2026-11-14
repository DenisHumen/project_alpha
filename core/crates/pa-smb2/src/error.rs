// SPDX-License-Identifier: MPL-2.0
//! Errors of libsmb2 calls, classified independently of the platform.

use std::fmt;

/// The libsmb2 call that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Op {
    /// Loading the shared library.
    Load,
    /// `smb2_init_context` or an option setter.
    Init,
    /// `smb2_connect_share`: TCP, negotiate, session setup, tree connect.
    Connect,
    /// `smb2_stat`.
    Stat,
    /// `smb2_opendir` / `smb2_readdir`.
    ListDir,
    /// `smb2_open`.
    Open,
    /// `smb2_fstat`.
    Fstat,
    /// `smb2_pread`.
    Read,
    /// `smb2_close`.
    Close,
}

/// What went wrong, the same on every OS (errno values differ between
/// platforms, so the classification uses the NT status first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// libsmb2 is not installed or is not a compatible build.
    LibraryUnavailable,
    /// The path or the share does not exist.
    NotFound,
    /// The server refused access to an existing path or share.
    PermissionDenied,
    /// Logon failed: wrong user or password, account disabled or locked.
    AuthenticationFailed,
    /// A directory operation on a file.
    NotADirectory,
    /// A file operation on a directory.
    IsADirectory,
    /// The server rejected the path syntax.
    InvalidPath,
    /// The server cannot be reached or dropped the connection.
    Unavailable,
    /// The operation did not finish within the context timeout.
    TimedOut,
    /// The server or libsmb2 does not support what was asked (dialect,
    /// signing or encryption requirement).
    Unsupported,
    /// Anything else; see the message.
    Other,
}

/// A failed libsmb2 call: the operation, the raw codes and libsmb2's message.
///
/// The message comes from `smb2_get_error` and never contains the password.
#[derive(Debug, Clone)]
pub struct Error {
    op: Op,
    kind: ErrorKind,
    errno: i32,
    nt_status: u32,
    message: String,
}

// NT status codes used for classification (MS-ERREF 2.3.1).
const STATUS_INVALID_PARAMETER: u32 = 0xC000_000D;
const STATUS_NO_SUCH_FILE: u32 = 0xC000_000F;
const STATUS_ACCESS_DENIED: u32 = 0xC000_0022;
const STATUS_OBJECT_NAME_INVALID: u32 = 0xC000_0033;
const STATUS_OBJECT_NAME_NOT_FOUND: u32 = 0xC000_0034;
const STATUS_OBJECT_PATH_INVALID: u32 = 0xC000_0039;
const STATUS_OBJECT_PATH_NOT_FOUND: u32 = 0xC000_003A;
const STATUS_OBJECT_PATH_SYNTAX_BAD: u32 = 0xC000_003B;
const STATUS_NO_SUCH_USER: u32 = 0xC000_0064;
const STATUS_WRONG_PASSWORD: u32 = 0xC000_006A;
const STATUS_LOGON_FAILURE: u32 = 0xC000_006D;
const STATUS_ACCOUNT_RESTRICTION: u32 = 0xC000_006E;
const STATUS_INVALID_LOGON_HOURS: u32 = 0xC000_006F;
const STATUS_PASSWORD_EXPIRED: u32 = 0xC000_0071;
const STATUS_ACCOUNT_DISABLED: u32 = 0xC000_0072;
const STATUS_IO_TIMEOUT: u32 = 0xC000_00B5;
const STATUS_FILE_IS_A_DIRECTORY: u32 = 0xC000_00BA;
const STATUS_NOT_SUPPORTED: u32 = 0xC000_00BB;
const STATUS_NETWORK_NAME_DELETED: u32 = 0xC000_00C9;
const STATUS_NETWORK_ACCESS_DENIED: u32 = 0xC000_00CA;
const STATUS_BAD_NETWORK_NAME: u32 = 0xC000_00CC;
const STATUS_NOT_A_DIRECTORY: u32 = 0xC000_0103;
const STATUS_PASSWORD_MUST_CHANGE: u32 = 0xC000_0224;
const STATUS_ACCOUNT_LOCKED_OUT: u32 = 0xC000_0234;
const STATUS_CONNECTION_DISCONNECTED: u32 = 0xC000_020C;
const STATUS_CONNECTION_RESET: u32 = 0xC000_020D;
const STATUS_CONNECTION_REFUSED: u32 = 0xC000_0236;
const STATUS_USER_SESSION_DELETED: u32 = 0xC000_0203;

impl Error {
    pub(crate) fn new(op: Op, errno: i32, nt_status: u32, message: String) -> Self {
        let kind = classify(op, errno, nt_status, &message);
        Self {
            op,
            kind,
            errno,
            nt_status,
            message,
        }
    }

    pub(crate) fn library(message: String) -> Self {
        Self {
            op: Op::Load,
            kind: ErrorKind::LibraryUnavailable,
            errno: 0,
            nt_status: 0,
            message,
        }
    }

    /// The failed operation.
    #[must_use]
    pub fn op(&self) -> Op {
        self.op
    }

    /// The classified kind.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The positive errno libsmb2 returned (platform specific), 0 if none.
    #[must_use]
    pub fn errno(&self) -> i32 {
        self.errno
    }

    /// The NT status of the failing reply, 0 if the failure was local
    /// (socket, timeout) or the server sent none.
    #[must_use]
    pub fn nt_status(&self) -> u32 {
        self.nt_status
    }

    /// libsmb2's description of the error.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} failed: {}", self.op, self.message)?;
        if self.nt_status != 0 {
            write!(f, " (NT status 0x{:08X})", self.nt_status)?;
        } else if self.errno != 0 {
            write!(f, " (errno {})", self.errno)?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

fn classify(op: Op, errno: i32, nt: u32, message: &str) -> ErrorKind {
    match nt {
        STATUS_LOGON_FAILURE
        | STATUS_WRONG_PASSWORD
        | STATUS_NO_SUCH_USER
        | STATUS_ACCOUNT_RESTRICTION
        | STATUS_INVALID_LOGON_HOURS
        | STATUS_PASSWORD_EXPIRED
        | STATUS_PASSWORD_MUST_CHANGE
        | STATUS_ACCOUNT_DISABLED
        | STATUS_ACCOUNT_LOCKED_OUT => return ErrorKind::AuthenticationFailed,
        STATUS_NO_SUCH_FILE
        | STATUS_OBJECT_NAME_NOT_FOUND
        | STATUS_OBJECT_PATH_NOT_FOUND
        | STATUS_BAD_NETWORK_NAME => return ErrorKind::NotFound,
        STATUS_ACCESS_DENIED | STATUS_NETWORK_ACCESS_DENIED => {
            return ErrorKind::PermissionDenied;
        }
        STATUS_NOT_A_DIRECTORY => return ErrorKind::NotADirectory,
        STATUS_FILE_IS_A_DIRECTORY => return ErrorKind::IsADirectory,
        STATUS_OBJECT_NAME_INVALID | STATUS_OBJECT_PATH_INVALID | STATUS_OBJECT_PATH_SYNTAX_BAD => {
            return ErrorKind::InvalidPath;
        }
        STATUS_IO_TIMEOUT => return ErrorKind::TimedOut,
        STATUS_NETWORK_NAME_DELETED
        | STATUS_CONNECTION_DISCONNECTED
        | STATUS_CONNECTION_RESET
        | STATUS_CONNECTION_REFUSED
        | STATUS_USER_SESSION_DELETED => return ErrorKind::Unavailable,
        STATUS_NOT_SUPPORTED => return ErrorKind::Unsupported,
        STATUS_INVALID_PARAMETER if op == Op::Open || op == Op::Stat => {
            return ErrorKind::InvalidPath;
        }
        _ => {}
    }
    // Local failures carry no NT status: libsmb2's sync loop reports a
    // timeout and socket errors in the message and the errno.
    let lower = message.to_ascii_lowercase();
    if lower.contains("timeout") || lower.contains("timed out") {
        return ErrorKind::TimedOut;
    }
    match errno_kind(errno) {
        ErrorKind::Other
            if op == Op::Connect
                && (lower.contains("socket")
                    || lower.contains("connect")
                    || lower.contains("resolve")) =>
        {
            ErrorKind::Unavailable
        }
        kind => kind,
    }
}

#[cfg(unix)]
fn errno_kind(errno: i32) -> ErrorKind {
    use std::io::ErrorKind as Io;
    if errno == 0 {
        return ErrorKind::Other;
    }
    match std::io::Error::from_raw_os_error(errno).kind() {
        Io::NotFound => ErrorKind::NotFound,
        Io::PermissionDenied => ErrorKind::PermissionDenied,
        Io::NotADirectory => ErrorKind::NotADirectory,
        Io::IsADirectory => ErrorKind::IsADirectory,
        Io::InvalidInput | Io::InvalidFilename => ErrorKind::InvalidPath,
        Io::TimedOut => ErrorKind::TimedOut,
        Io::ConnectionRefused
        | Io::ConnectionReset
        | Io::ConnectionAborted
        | Io::NotConnected
        | Io::HostUnreachable
        | Io::NetworkUnreachable
        | Io::NetworkDown
        | Io::BrokenPipe
        | Io::AddrNotAvailable => ErrorKind::Unavailable,
        Io::Unsupported => ErrorKind::Unsupported,
        _ => ErrorKind::Other,
    }
}

/// libsmb2 on Windows (mingw-w64, msvcrt) returns CRT errno values, with
/// socket errors mapped to `WSAE*` codes by its compat layer.
#[cfg(not(unix))]
fn errno_kind(errno: i32) -> ErrorKind {
    match errno {
        2 => ErrorKind::NotFound,              // ENOENT
        1 | 13 => ErrorKind::PermissionDenied, // EPERM, EACCES
        20 => ErrorKind::NotADirectory,        // ENOTDIR
        21 => ErrorKind::IsADirectory,         // EISDIR
        22 => ErrorKind::InvalidPath,          // EINVAL
        10060 | 138 => ErrorKind::TimedOut,    // WSAETIMEDOUT, ETIMEDOUT
        10050..=10054 | 10057 | 10061 | 10064 | 10065 => ErrorKind::Unavailable, // WSAENETDOWN..WSAECONNRESET, WSAENOTCONN, WSAECONNREFUSED, WSAEHOSTDOWN, WSAEHOSTUNREACH
        _ => ErrorKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nt_status_wins_over_errno() {
        // libsmb2 maps LOGON_FAILURE to ECONNREFUSED; it is an auth error.
        let e = Error::new(
            Op::Connect,
            111,
            STATUS_LOGON_FAILURE,
            "Session setup failed".into(),
        );
        assert_eq!(e.kind(), ErrorKind::AuthenticationFailed);
        let e = Error::new(
            Op::Connect,
            2,
            STATUS_BAD_NETWORK_NAME,
            "Tree connect".into(),
        );
        assert_eq!(e.kind(), ErrorKind::NotFound);
        let e = Error::new(Op::Open, 1, STATUS_FILE_IS_A_DIRECTORY, "open".into());
        assert_eq!(e.kind(), ErrorKind::IsADirectory);
        let e = Error::new(Op::ListDir, 20, STATUS_NOT_A_DIRECTORY, "opendir".into());
        assert_eq!(e.kind(), ErrorKind::NotADirectory);
        assert!(e.to_string().contains("0xC0000103"), "{e}");
    }

    #[test]
    fn local_failures() {
        let e = Error::new(
            Op::Read,
            0,
            0,
            "Timeout expired and no connection exists".into(),
        );
        assert_eq!(e.kind(), ErrorKind::TimedOut);
        let e = Error::new(
            Op::Connect,
            0,
            0,
            "smb2_service failed with : Socket connect failed with 111".into(),
        );
        assert_eq!(e.kind(), ErrorKind::Unavailable);
        assert_eq!(
            Error::library("x".into()).kind(),
            ErrorKind::LibraryUnavailable
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_errno() {
        assert_eq!(errno_kind(2), ErrorKind::NotFound);
        assert_eq!(errno_kind(111), ErrorKind::Unavailable);
        assert_eq!(errno_kind(110), ErrorKind::TimedOut);
        assert_eq!(errno_kind(0), ErrorKind::Other);
    }
}
