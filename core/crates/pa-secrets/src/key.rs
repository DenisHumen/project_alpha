// SPDX-License-Identifier: MPL-2.0
//! Names of secrets.

use std::fmt;

use crate::{Result, SecretError};

/// Maximum length of a [`SecretKey`] in bytes.
pub const MAX_KEY_LEN: usize = 128;

/// Name of a secret, for example `source.password:3f1c…` or
/// `twitch.tokens:12345`.
///
/// Keys are ASCII `[A-Za-z0-9._:@-]`, 1 to [`MAX_KEY_LEN`] bytes. The key is
/// stored in the clear as the account name of the keychain item, so it must
/// not contain the secret itself or personal data such as an email: use the
/// internal id of the source or account.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretKey(String);

impl SecretKey {
    /// Validates and wraps a key.
    pub fn new(key: impl Into<String>) -> Result<Self> {
        let key = key.into();
        let valid = !key.is_empty()
            && key.len() <= MAX_KEY_LEN
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'@' | b'-'));
        if valid {
            Ok(Self(key))
        } else {
            Err(SecretError::InvalidKey(key))
        }
    }

    /// Password of a network source (SMB, WebDAV, NFS, Plex, Jellyfin) by
    /// the source id.
    pub fn source_password(source_id: &str) -> Result<Self> {
        Self::new(format!("source.password:{source_id}"))
    }

    /// OAuth tokens (access and refresh, usually as one JSON document) of a
    /// Twitch account on this device, by the Twitch user id.
    pub fn twitch_tokens(user_id: &str) -> Result<Self> {
        Self::new(format!("twitch.tokens:{user_id}"))
    }

    /// OAuth tokens of a Trakt account, by our account id.
    pub fn trakt_tokens(account_id: &str) -> Result<Self> {
        Self::new(format!("trakt.tokens:{account_id}"))
    }

    /// The user's own API key of a metadata provider (`tmdb`, `fanart`,
    /// `opensubtitles`, `tvdb`).
    pub fn provider_api_key(provider: &str) -> Result<Self> {
        Self::new(format!("provider.api_key:{provider}"))
    }

    /// The user's own TMDB API key.
    pub fn tmdb_api_key() -> Self {
        Self("provider.api_key:tmdb".to_owned())
    }

    /// The key as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey({:?})", self.0)
    }
}

impl fmt::Display for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SecretKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_keys() {
        for key in [
            "a",
            "source.password:3f1c0b2e-9d4a-4c1e-8f00-1234567890ab",
            "twitch.tokens:12345",
            "user@host",
            "A_B-c.d:e",
        ] {
            assert_eq!(SecretKey::new(key).unwrap().as_str(), key);
        }
        assert_eq!(SecretKey::tmdb_api_key().as_str(), "provider.api_key:tmdb");
        assert_eq!(
            SecretKey::provider_api_key("tmdb").unwrap(),
            SecretKey::tmdb_api_key()
        );
        SecretKey::new("k".repeat(MAX_KEY_LEN)).unwrap();
    }

    #[test]
    fn rejects_invalid_keys() {
        for key in [
            String::new(),
            "k".repeat(MAX_KEY_LEN + 1),
            "with space".into(),
            "slash/".into(),
            "кириллица".into(),
            "nul\0".into(),
        ] {
            assert!(
                matches!(SecretKey::new(key.clone()), Err(SecretError::InvalidKey(k)) if k == key),
                "{key:?} accepted"
            );
        }
        assert!(SecretKey::source_password("a b").is_err());
    }
}
