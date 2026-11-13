// SPDX-License-Identifier: MPL-2.0
//! Backend-neutral error type.

use std::error::Error as StdError;
use std::fmt;
use std::io;

/// Result alias used across `pa-vfs`.
pub type Result<T> = std::result::Result<T, VfsError>;

/// What went wrong, independent of the protocol.
///
/// Backends map their native errors (errno, NTSTATUS, HTTP status, SFTP
/// status codes) to these kinds, so callers such as the scanner and the
/// source manager react the same way to every source. The enum is
/// `non_exhaustive`: new kinds can be added without breaking matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The path does not exist.
    NotFound,
    /// The server or the OS refused access to an existing path.
    PermissionDenied,
    /// A directory operation (list) was called on something that is not a directory.
    NotADirectory,
    /// A file operation (open) was called on a directory.
    IsADirectory,
    /// The path cannot be represented on this backend (bad characters,
    /// reserved names, `..` escaping the source root).
    InvalidPath,
    /// Credentials were missing or rejected.
    AuthenticationFailed,
    /// The source cannot be reached right now (connection refused or reset,
    /// host down, share offline). Retrying later may succeed.
    Unavailable,
    /// The operation did not finish in time.
    TimedOut,
    /// The backend does not support the operation.
    Unsupported,
    /// Anything else; see the message and the source error.
    Other,
}

impl ErrorKind {
    /// `true` for transient failures a caller may retry
    /// ([`Unavailable`](Self::Unavailable), [`TimedOut`](Self::TimedOut)).
    #[must_use]
    pub fn is_transient(self) -> bool {
        matches!(self, Self::Unavailable | Self::TimedOut)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not found",
            Self::PermissionDenied => "permission denied",
            Self::NotADirectory => "not a directory",
            Self::IsADirectory => "is a directory",
            Self::InvalidPath => "invalid path",
            Self::AuthenticationFailed => "authentication failed",
            Self::Unavailable => "source unavailable",
            Self::TimedOut => "timed out",
            Self::Unsupported => "unsupported",
            Self::Other => "error",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned by [`Vfs`](crate::Vfs) and [`ByteSource`](crate::ByteSource).
///
/// The message must not contain secrets: backends put paths and server
/// replies here, never passwords or tokens.
#[derive(Debug)]
pub struct VfsError {
    kind: ErrorKind,
    message: String,
    source: Option<Box<dyn StdError + Send + Sync + 'static>>,
}

impl VfsError {
    /// Creates an error with a kind and a human-readable message.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
        }
    }

    /// Attaches the underlying cause.
    #[must_use]
    pub fn with_source(mut self, source: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Maps an [`io::Error`] to a [`VfsError`], adding `context` (usually the path).
    pub fn from_io(err: io::Error, context: impl fmt::Display) -> Self {
        let kind = match err.kind() {
            io::ErrorKind::NotFound => ErrorKind::NotFound,
            io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
            io::ErrorKind::NotADirectory => ErrorKind::NotADirectory,
            io::ErrorKind::IsADirectory => ErrorKind::IsADirectory,
            io::ErrorKind::InvalidFilename | io::ErrorKind::InvalidInput => ErrorKind::InvalidPath,
            io::ErrorKind::TimedOut => ErrorKind::TimedOut,
            io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::NetworkDown
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::StaleNetworkFileHandle => ErrorKind::Unavailable,
            io::ErrorKind::Unsupported => ErrorKind::Unsupported,
            _ => ErrorKind::Other,
        };
        Self::new(kind, format!("{context}: {err}")).with_source(err)
    }

    /// The error kind.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The message without the kind prefix.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl StdError for VfsError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_deref()
            .map(|e| e as &(dyn StdError + 'static))
    }
}

impl From<VfsError> for io::Error {
    /// Lets [`ByteSource`](crate::ByteSource) adapters (FFmpeg AVIO, mpv
    /// stream callbacks, `std::io::Read`) report errors through `io::Error`.
    fn from(err: VfsError) -> Self {
        let kind = match err.kind {
            ErrorKind::NotFound => io::ErrorKind::NotFound,
            ErrorKind::PermissionDenied | ErrorKind::AuthenticationFailed => {
                io::ErrorKind::PermissionDenied
            }
            ErrorKind::NotADirectory => io::ErrorKind::NotADirectory,
            ErrorKind::IsADirectory => io::ErrorKind::IsADirectory,
            ErrorKind::InvalidPath => io::ErrorKind::InvalidInput,
            ErrorKind::Unavailable => io::ErrorKind::NotConnected,
            ErrorKind::TimedOut => io::ErrorKind::TimedOut,
            ErrorKind::Unsupported => io::ErrorKind::Unsupported,
            ErrorKind::Other => io::ErrorKind::Other,
        };
        io::Error::new(kind, err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_kinds_map_both_ways() {
        let e = VfsError::from_io(io::Error::from(io::ErrorKind::NotFound), "/a");
        assert_eq!(e.kind(), ErrorKind::NotFound);
        assert!(e.to_string().starts_with("not found: /a"));
        assert!(e.source().is_some());
        let back: io::Error = e.into();
        assert_eq!(back.kind(), io::ErrorKind::NotFound);

        let e = VfsError::from_io(io::Error::from(io::ErrorKind::ConnectionReset), "/b");
        assert_eq!(e.kind(), ErrorKind::Unavailable);
        assert!(e.kind().is_transient());
        assert!(!ErrorKind::NotFound.is_transient());
    }
}
