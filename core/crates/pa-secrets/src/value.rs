// SPDX-License-Identifier: MPL-2.0
//! Secret values that do not leak through `Debug` and are zeroed on drop.

use std::fmt;

use zeroize::Zeroizing;

use crate::{Result, SecretError};

/// Maximum length of a secret value in bytes.
///
/// Windows Credential Manager limits a credential to 2560 bytes; values are
/// stored there as UTF-16, so the portable limit is 1280 bytes of ASCII.
/// OAuth token documents (access + refresh + expiry) fit well below.
pub const MAX_SECRET_LEN: usize = 1280;

/// A secret value (password, token, API key).
///
/// The memory is zeroed when the value is dropped. `Debug` prints `***`;
/// there is no `Display`, the text is reachable only through
/// [`SecretString::expose`].
#[derive(Clone, PartialEq, Eq, Default)]
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    /// Wraps a value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// The secret text. Do not log it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the value is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Fails with [`SecretError::TooLarge`] above [`MAX_SECRET_LEN`].
    pub(crate) fn check_len(&self) -> Result<()> {
        if self.len() > MAX_SECRET_LEN {
            Err(SecretError::TooLarge {
                len: self.len(),
                max: MAX_SECRET_LEN,
            })
        } else {
            Ok(())
        }
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(***)")
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_value() {
        let secret = SecretString::new("hunter2");
        assert_eq!(format!("{secret:?}"), "SecretString(***)");
        assert_eq!(format!("{:?}", Some(&secret)), "Some(SecretString(***))");
        assert_eq!(secret.expose(), "hunter2");
        assert_eq!(secret.len(), 7);
    }

    #[test]
    fn length_limit() {
        SecretString::new("x".repeat(MAX_SECRET_LEN))
            .check_len()
            .unwrap();
        assert!(matches!(
            SecretString::new("x".repeat(MAX_SECRET_LEN + 1)).check_len(),
            Err(SecretError::TooLarge { .. })
        ));
    }
}
