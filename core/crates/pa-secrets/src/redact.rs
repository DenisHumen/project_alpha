// SPDX-License-Identifier: MPL-2.0
//! Masking secrets in log lines and diagnostic reports.
//!
//! [`redact`] replaces with `***`:
//!
//! 1. every secret value that went through a store opened with
//!    [`open`](crate::open) in this process (see [`register_known`]);
//! 2. values of credential-like fields: `password=…`, `"token": "…"`,
//!    `access_token=…`, `client_secret=…`, `api_key=…` and similar;
//! 3. `Bearer …`, `Basic …` and `OAuth …` authorization values;
//! 4. the password part of URLs: `smb://user:pass@host` →
//!    `smb://user:***@host`.
//!
//! The logger (CORE-008) runs every message through [`redact`] before it
//! is written.

use std::sync::{PoisonError, RwLock};

use zeroize::Zeroizing;

use crate::SecretString;

/// Shorter known values are not matched: they would mask ordinary words.
/// Field patterns still catch them.
const MIN_KNOWN_LEN: usize = 4;
const MASK: &str = "***";

/// Field names whose values are masked (lowercase).
const FIELDS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "pass",
    "secret",
    "client_secret",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "device_code",
    "api_key",
    "apikey",
    "x-api-key",
    "auth_token",
    "oauth_token",
    "session",
    "sessionid",
    "cookie",
];

/// Authorization schemes whose following word is masked (lowercase, with
/// the trailing space).
const SCHEMES: &[&str] = &["bearer ", "basic ", "oauth ", "digest "];

static KNOWN: RwLock<Vec<Zeroizing<String>>> = RwLock::new(Vec::new());

/// Remembers a secret value so that [`redact`] masks it.
pub fn register_known(value: &SecretString) {
    if value.len() < MIN_KNOWN_LEN {
        return;
    }
    let mut known = KNOWN.write().unwrap_or_else(PoisonError::into_inner);
    if !known.iter().any(|k| k.as_str() == value.expose()) {
        known.push(Zeroizing::new(value.expose().to_owned()));
        // Longer values first, so a value that contains another is masked whole.
        known.sort_by_key(|k| std::cmp::Reverse(k.len()));
    }
}

/// Forgets a value registered with [`register_known`].
pub fn forget_known(value: &SecretString) {
    KNOWN
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|k| k.as_str() != value.expose());
}

/// Returns `text` with secrets replaced by `***`.
pub fn redact(text: &str) -> String {
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    {
        let known = KNOWN.read().unwrap_or_else(PoisonError::into_inner);
        for value in known.iter() {
            for (start, matched) in text.match_indices(value.as_str()) {
                ranges.push((start, start + matched.len()));
            }
        }
    }

    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for start in 0..bytes.len() {
        if start > 0 && is_word(bytes[start - 1]) {
            continue;
        }
        if let Some(range) = field_value(bytes, start) {
            ranges.push(range);
        } else if let Some(range) = scheme_value(bytes, start) {
            ranges.push(range);
        }
    }
    url_passwords(bytes, &mut ranges);

    if ranges.is_empty() {
        return text.to_owned();
    }
    ranges.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for (start, end) in ranges {
        if end <= pos {
            continue;
        }
        let start = start.max(pos);
        out.push_str(&text[pos..start]);
        out.push_str(MASK);
        pos = end;
    }
    out.push_str(&text[pos..]);
    out
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// Value delimiters: everything else belongs to the value.
fn ends_value(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b'&' | b'"' | b'\'' | b',' | b';' | b'}' | b')' | b']')
}

/// `name=value`, `name: value`, `"name": "value"` starting at `start`.
fn field_value(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let name = FIELDS
        .iter()
        .filter(|f| bytes[start..].starts_with(f.as_bytes()))
        .max_by_key(|f| f.len())?;
    let mut i = start + name.len();
    if i < bytes.len() && is_word(bytes[i]) {
        return None;
    }
    if bytes.get(i) == Some(&b'"') || bytes.get(i) == Some(&b'\'') {
        i += 1;
    }
    while bytes.get(i) == Some(&b' ') {
        i += 1;
    }
    if !matches!(bytes.get(i), Some(b'=' | b':')) {
        return None;
    }
    i += 1;
    while matches!(bytes.get(i), Some(b' ' | b'"' | b'\'')) {
        i += 1;
    }
    // `"password": null` and `"token": 0` carry nothing to hide; masking
    // them is harmless, but an empty value is left as is.
    let value_start = i;
    while i < bytes.len() && !ends_value(bytes[i]) {
        i += 1;
    }
    (i > value_start).then_some((value_start, i))
}

/// `Bearer <token>` and other authorization schemes.
fn scheme_value(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let scheme = SCHEMES
        .iter()
        .find(|s| bytes[start..].starts_with(s.as_bytes()))?;
    let value_start = start + scheme.len();
    let mut i = value_start;
    while i < bytes.len() && !ends_value(bytes[i]) {
        i += 1;
    }
    // A token is at least 8 characters; this keeps "basic usage" readable.
    (i - value_start >= 8).then_some((value_start, i))
}

/// `scheme://user:password@host` → masks `password`.
fn url_passwords(bytes: &[u8], ranges: &mut Vec<(usize, usize)>) {
    let mut from = 0;
    while let Some(found) = find(&bytes[from..], b"://") {
        let auth_start = from + found + 3;
        let mut i = auth_start;
        let mut colon = None;
        while i < bytes.len()
            && !matches!(bytes[i], b'/' | b'@' | b'?' | b'#')
            && !ends_value(bytes[i])
        {
            if bytes[i] == b':' && colon.is_none() {
                colon = Some(i);
            }
            i += 1;
        }
        if bytes.get(i) == Some(&b'@')
            && let Some(colon) = colon
            && i > colon + 1
        {
            ranges.push((colon + 1, i));
        }
        from = auth_start;
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_fields() {
        let cases = [
            (
                "login user=bob password=hunter2 ok",
                "login user=bob password=*** ok",
            ),
            (
                "?client_id=abc&access_token=xyz123&x=1",
                "?client_id=abc&access_token=***&x=1",
            ),
            (
                r#"{"refresh_token": "r-1", "expires_in": 3600}"#,
                r#"{"refresh_token": "***", "expires_in": 3600}"#,
            ),
            ("Password: s3cr3t", "Password: ***"),
            ("api_key=K&apikey=L", "api_key=***&apikey=***"),
            ("pass=1", "pass=***"),
            (
                "passport=123 bypass=1 tokens=2",
                "passport=123 bypass=1 tokens=2",
            ),
            ("password=", "password="),
            ("no secrets here", "no secrets here"),
        ];
        for (input, expected) in cases {
            assert_eq!(redact(input), expected, "{input}");
        }
    }

    #[test]
    fn masks_authorization_schemes() {
        assert_eq!(
            redact("Authorization: Bearer abcdefgh12345"),
            "Authorization: Bearer ***"
        );
        assert_eq!(
            redact("header authorization: OAuth 0123456789abcdef"),
            "header authorization: OAuth ***"
        );
        assert_eq!(redact("basic usage"), "basic usage");
    }

    #[test]
    fn masks_url_passwords() {
        assert_eq!(
            redact("open smb://bob:p%40ss@nas.local/share failed"),
            "open smb://bob:***@nas.local/share failed"
        );
        assert_eq!(
            redact("https://example.org:8443/path ok"),
            "https://example.org:8443/path ok"
        );
        assert_eq!(redact("webdav://user@host/"), "webdav://user@host/");
    }

    #[test]
    fn masks_known_values() {
        let secret = SecretString::new("kJ8sQ2pLx9");
        register_known(&secret);
        register_known(&SecretString::new("abc")); // too short, ignored
        assert_eq!(
            redact("token kJ8sQ2pLx9 used twice: kJ8sQ2pLx9."),
            "token *** used twice: ***."
        );
        assert_eq!(redact("abc"), "abc");
        forget_known(&secret);
        assert_eq!(redact("kJ8sQ2pLx9"), "kJ8sQ2pLx9");
    }

    #[test]
    fn keeps_utf8_intact() {
        assert_eq!(
            redact("пароль password=пароль123 конец"),
            "пароль password=*** конец"
        );
    }
}
