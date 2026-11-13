// SPDX-License-Identifier: MPL-2.0
//! Attribution each provider requires, for the "About / Credits" screen
//! (UI-016) and wherever provider data is shown.
//!
//! Logos are not bundled here: UI-016 uses the provider's approved assets
//! under its brand guidelines (docs/legal/providers.md).

/// What the UI must show for one provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attribution {
    /// Provider id, as in `ProviderRequest::provider`.
    pub provider: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Required notice, verbatim (not translated).
    pub notice: &'static str,
    /// Link to the provider's site.
    pub url: &'static str,
    /// Where the approved logos and their usage rules are published.
    pub logo_guidelines_url: Option<&'static str>,
}

/// TMDB: approved logo, less prominent than our own brand, plus this exact
/// sentence (TMDB API Terms of Use; docs/legal/providers.md, TMDB §4).
pub const TMDB: Attribution = Attribution {
    provider: "tmdb",
    name: "TMDB",
    notice: "This product uses the TMDB API but is not endorsed or certified by TMDB.",
    url: "https://www.themoviedb.org/",
    logo_guidelines_url: Some("https://www.themoviedb.org/about/logos-attribution"),
};

/// Attributions of all providers implemented in this crate.
pub const ALL: &[Attribution] = &[TMDB];
