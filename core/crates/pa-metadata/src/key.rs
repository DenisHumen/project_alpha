// SPDX-License-Identifier: MPL-2.0
//! API keys: the project key baked in at build time and the user's own key
//! (ADR-0011 §6, DEC-008; docs/legal/providers.md §1).
//!
//! Rules:
//! - The user's key, when set, always wins over the project key.
//! - The project key is read at **compile time** from `PA_TMDB_API_KEY`
//!   (GitHub Secrets of the `release`/`store` environments, or a developer's
//!   `.env.local`). It never lives in git. Builds from source without it
//!   (F-Droid, contributors) simply have no project key.
//! - Without any key the client answers [`TmdbError::NoKey`](crate::tmdb::TmdbError)
//!   (or serves the cache) and the app keeps working on recognised names.
//! - Keys are redacted in `Debug` output and never enter cache keys,
//!   fixtures or recordings.

use std::fmt;

/// Placeholder values from `.env.example` that mean "no key".
const PLACEHOLDERS: &[&str] = &["changeme", "CHANGEME", "your-key-here"];

/// A secret API credential.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wraps a key; `None` for empty input and `.env.example` placeholders.
    pub fn new(raw: &str) -> Option<Self> {
        let key = raw.trim();
        (!key.is_empty() && !PLACEHOLDERS.contains(&key)).then(|| Self(key.to_owned()))
    }

    /// TMDB accepts two kinds of credentials: the v3 API key (32 hex
    /// characters, sent as `api_key=`) and the v4 API Read Access Token
    /// (a JWT, sent as `Authorization: Bearer`).
    pub fn is_bearer_token(&self) -> bool {
        self.0.starts_with("eyJ") && self.0.matches('.').count() == 2
    }

    /// The secret itself. Only the HTTP layer should call this.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// Where the active key came from (for the settings screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// Entered by the user ("свой ключ").
    User,
    /// Shipped with the build.
    Project,
}

/// Project key plus optional user override.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keys {
    project: Option<ApiKey>,
    user: Option<ApiKey>,
}

impl Keys {
    /// No key at all.
    pub fn none() -> Self {
        Self::default()
    }

    /// The TMDB project key compiled into this build, if any.
    pub fn tmdb_from_build() -> Self {
        Self {
            project: option_env!("PA_TMDB_API_KEY").and_then(ApiKey::new),
            user: None,
        }
    }

    /// Replaces the project key (tests, tools, server config).
    pub fn with_project_key(mut self, key: Option<&str>) -> Self {
        self.project = key.and_then(ApiKey::new);
        self
    }

    /// Sets or clears the user's key.
    pub fn with_user_key(mut self, key: Option<&str>) -> Self {
        self.user = key.and_then(ApiKey::new);
        self
    }

    /// The key to use now: the user's if set, else the project's.
    pub fn active(&self) -> Option<(&ApiKey, KeySource)> {
        self.user
            .as_ref()
            .map(|k| (k, KeySource::User))
            .or_else(|| self.project.as_ref().map(|k| (k, KeySource::Project)))
    }

    /// True when the build ships a project key.
    pub fn has_project_key(&self) -> bool {
        self.project.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_key_overrides_project_key() {
        let keys = Keys::none().with_project_key(Some("project"));
        assert_eq!(keys.active().unwrap().1, KeySource::Project);
        let keys = keys.with_user_key(Some(" mine "));
        let (key, source) = keys.active().unwrap();
        assert_eq!((key.expose(), source), ("mine", KeySource::User));
        let keys = keys.with_user_key(None);
        assert_eq!(keys.active().unwrap().1, KeySource::Project);
    }

    #[test]
    fn placeholders_and_blank_are_no_key() {
        assert!(ApiKey::new("").is_none());
        assert!(ApiKey::new("  ").is_none());
        assert!(ApiKey::new("changeme").is_none());
        assert!(
            Keys::none()
                .with_user_key(Some("changeme"))
                .active()
                .is_none()
        );
    }

    #[test]
    fn debug_is_redacted() {
        let keys = Keys::none().with_user_key(Some("super-secret"));
        let shown = format!("{keys:?}");
        assert!(!shown.contains("super-secret"), "{shown}");
    }

    #[test]
    fn bearer_detection() {
        // Shapes only; real tokens are much longer.
        assert!(ApiKey::new("eyJx.eyJy.z").unwrap().is_bearer_token());
        assert!(!ApiKey::new("0123abcd").unwrap().is_bearer_token());
    }
}
