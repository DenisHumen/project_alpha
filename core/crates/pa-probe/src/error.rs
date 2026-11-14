// SPDX-License-Identifier: MPL-2.0
//! Probe errors.

use std::fmt;

/// Why a probe failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProbeErrorKind {
    /// FFmpeg is not installed or is not a supported version. Not a
    /// property of the file: probe it again once FFmpeg is available.
    LibraryUnavailable,
    /// The source failed (network, permissions…); see
    /// [`ProbeError::source_kind`]. Transient kinds are worth a retry.
    Source,
    /// libavformat does not recognise the data within the read window:
    /// not a media file, truncated, or its index lies outside the window.
    InvalidData,
    /// The probe was cancelled.
    Cancelled,
}

/// A failed probe: the kind, the source error kind if any, and a message
/// (no secrets, no full paths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeError {
    kind: ProbeErrorKind,
    source_kind: Option<pa_vfs::ErrorKind>,
    message: String,
}

impl ProbeError {
    pub(crate) fn new(kind: ProbeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            source_kind: None,
            message: message.into(),
        }
    }

    pub(crate) fn from_vfs(err: &pa_vfs::VfsError) -> Self {
        Self {
            kind: ProbeErrorKind::Source,
            source_kind: Some(err.kind()),
            message: err.to_string(),
        }
    }

    /// What went wrong.
    #[must_use]
    pub fn kind(&self) -> ProbeErrorKind {
        self.kind
    }

    /// For [`ProbeErrorKind::Source`]: the protocol-neutral source error.
    #[must_use]
    pub fn source_kind(&self) -> Option<pa_vfs::ErrorKind> {
        self.source_kind
    }

    /// Human-readable details.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// `true` if probing the same file again later may succeed: the library
    /// is missing, the source failed transiently, or the probe was cancelled.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self.kind {
            ProbeErrorKind::LibraryUnavailable | ProbeErrorKind::Cancelled => true,
            ProbeErrorKind::Source => self.source_kind.is_none_or(pa_vfs::ErrorKind::is_transient),
            ProbeErrorKind::InvalidData => false,
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProbeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_vfs::{ErrorKind, VfsError};

    #[test]
    fn retryable_kinds() {
        assert!(ProbeError::new(ProbeErrorKind::LibraryUnavailable, "x").is_retryable());
        assert!(!ProbeError::new(ProbeErrorKind::InvalidData, "x").is_retryable());
        let e = ProbeError::from_vfs(&VfsError::new(ErrorKind::TimedOut, "slow"));
        assert_eq!(e.kind(), ProbeErrorKind::Source);
        assert_eq!(e.source_kind(), Some(ErrorKind::TimedOut));
        assert!(e.is_retryable());
        assert!(
            !ProbeError::from_vfs(&VfsError::new(ErrorKind::PermissionDenied, "no")).is_retryable()
        );
    }
}
