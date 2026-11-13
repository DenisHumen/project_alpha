// SPDX-License-Identifier: MPL-2.0
//! Mapping between [`VfsPath`] and server URLs.
//!
//! Requests percent-encode every byte of a name except RFC 3986 unreserved
//! characters, so `#`, `%`, `&`, spaces and Cyrillic survive any server.
//! Hrefs in PROPFIND replies are compared segment by segment after
//! percent-decoding each segment on its own: servers differ in which
//! characters they escape and in hex case, and a `%2F` inside a name must not
//! become a separator.

use pa_vfs::{ErrorKind, Result, VfsError, VfsPath};
use reqwest::Url;

/// Percent-encodes one path segment (everything but `A–Z a–z 0–9 - . _ ~`).
pub(crate) fn encode_segment(name: &str, out: &mut String) {
    for &b in name.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 0xf)]));
        }
    }
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Percent-decodes one segment; `None` if the bytes are not UTF-8.
/// Malformed escapes (`%zz`, a trailing `%`) are kept literally.
pub(crate) fn decode_segment(seg: &str) -> Option<String> {
    let bytes = seg.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(h), Some(l)) = (
                bytes.get(i + 1).and_then(|c| char::from(*c).to_digit(16)),
                bytes.get(i + 2).and_then(|c| char::from(*c).to_digit(16)),
            )
        {
            out.push(u8::try_from(h * 16 + l).unwrap_or(0));
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// Decoded, non-empty segments of a URL path.
pub(crate) fn decoded_segments(path: &str) -> Option<Vec<String>> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(decode_segment)
        .collect()
}

/// The source root and helpers to build request URLs under it.
#[derive(Debug, Clone)]
pub(crate) struct Base {
    /// Root collection URL, path ends with `/`.
    url: Url,
    /// Decoded segments of the root path.
    segments: Vec<String>,
}

impl Base {
    /// Validates and normalizes the root URL of a source.
    pub(crate) fn parse(url: &str) -> Result<Self> {
        let invalid = |why: &str| VfsError::new(ErrorKind::InvalidPath, format!("WebDAV URL: {why}"));
        let mut url = Url::parse(url).map_err(|e| invalid(&e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(invalid("scheme must be http or https"));
        }
        if url.host_str().is_none() {
            return Err(invalid("no host"));
        }
        // Credentials go to `Credentials`, never into a URL that may be logged.
        if !url.username().is_empty() || url.password().is_some() {
            return Err(invalid("user name or password in the URL; pass them as credentials"));
        }
        url.set_query(None);
        url.set_fragment(None);
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        let segments = decoded_segments(url.path()).ok_or_else(|| invalid("path is not UTF-8"))?;
        Ok(Self { url, segments })
    }

    /// The root URL.
    pub(crate) fn url(&self) -> &Url {
        &self.url
    }

    /// URL of `path`; directories may get a trailing slash with `dir`.
    pub(crate) fn url_of(&self, path: &VfsPath, dir: bool) -> Url {
        let mut s = String::from(self.url.as_str());
        for (i, c) in path.components().enumerate() {
            if i > 0 {
                s.push('/');
            }
            encode_segment(c, &mut s);
        }
        if dir && !path.is_root() {
            s.push('/');
        }
        // Only unreserved characters and escapes were appended to a valid URL.
        Url::parse(&s).unwrap_or_else(|_| self.url.clone())
    }

    /// Same scheme, host and port as the root: credentials may be sent.
    pub(crate) fn same_origin(&self, url: &Url) -> bool {
        url.origin() == self.url.origin()
    }

    /// Components of `href` (absolute URL or absolute path, as in a
    /// multistatus reply) below the root; `None` if it is outside the root,
    /// on another origin or not UTF-8.
    pub(crate) fn relative(&self, href: &str) -> Option<Vec<String>> {
        let url = self.url.join(href.trim()).ok()?;
        if !self.same_origin(&url) {
            return None;
        }
        let segs = decoded_segments(url.path())?;
        segs.starts_with(&self.segments)
            .then(|| segs[self.segments.len()..].to_vec())
    }

    /// The root as seen by a reply to a request for `path` that ended at
    /// `reply_url` after redirects: hrefs in it are relative to the moved
    /// location. Without redirects (or if the target does not end with
    /// `path`) this is the configured root.
    pub(crate) fn as_seen_by(&self, reply_url: &Url, path: &VfsPath) -> Self {
        if reply_url.origin() == self.url.origin() && reply_url.path() == self.url_of(path, false).path() {
            return self.clone();
        }
        let comps: Vec<&str> = path.components().collect();
        match decoded_segments(reply_url.path()) {
            Some(segs) if segs.len() >= comps.len() && segs[segs.len() - comps.len()..] == comps[..] => {
                let mut url = reply_url.clone();
                url.set_query(None);
                Self { url, segments: segs[..segs.len() - comps.len()].to_vec() }
            }
            _ => self.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> VfsPath {
        VfsPath::parse(s).unwrap()
    }

    #[test]
    fn encodes_every_reserved_byte() {
        let mut s = String::new();
        encode_segment("special #1 & 100% 'q'.txt", &mut s);
        assert_eq!(s, "special%20%231%20%26%20100%25%20%27q%27.txt");
        assert_eq!(decode_segment(&s).unwrap(), "special #1 & 100% 'q'.txt");
        let mut s = String::new();
        encode_segment("Дюна", &mut s);
        assert_eq!(s, "%D0%94%D1%8E%D0%BD%D0%B0");
    }

    #[test]
    fn decode_is_lenient() {
        assert_eq!(decode_segment("a%2fb").unwrap(), "a/b");
        assert_eq!(decode_segment("100%").unwrap(), "100%");
        assert_eq!(decode_segment("%zz").unwrap(), "%zz");
        assert_eq!(decode_segment("Дюна").unwrap(), "Дюна");
        assert!(decode_segment("%FF").is_none());
    }

    #[test]
    fn base_is_normalized() {
        let b = Base::parse("https://nas.local:8443/remote.php/dav/files/u?x=1#f").unwrap();
        assert_eq!(b.url().as_str(), "https://nas.local:8443/remote.php/dav/files/u/");
        assert!(Base::parse("ftp://h/").is_err());
        assert!(Base::parse("http://user:pw@h/").is_err());
        assert!(Base::parse("not a url").is_err());
    }

    #[test]
    fn urls_of_paths() {
        let b = Base::parse("http://h/dav").unwrap();
        assert_eq!(b.url_of(&VfsPath::root(), true).as_str(), "http://h/dav/");
        assert_eq!(b.url_of(&p("/a b/c#"), false).as_str(), "http://h/dav/a%20b/c%23");
        assert_eq!(b.url_of(&p("/a"), true).as_str(), "http://h/dav/a/");
    }

    #[test]
    fn relative_hrefs() {
        let b = Base::parse("http://h/dav/files/").unwrap();
        let rel = |h| b.relative(h);
        assert_eq!(rel("/dav/files/"), Some(vec![]));
        assert_eq!(rel("http://h/dav/files/a%20b/"), Some(vec!["a b".to_owned()]));
        assert_eq!(rel("/dav/files/a/%D0%94"), Some(vec!["a".to_owned(), "Д".to_owned()]));
        // Unescaped non-ASCII and lowercase hex.
        assert_eq!(rel("/dav/files/Дюна%2fx"), Some(vec!["Дюна/x".to_owned()]));
        assert_eq!(rel("/dav/other/"), None);
        assert_eq!(rel("http://evil/dav/files/a"), None);
        // Base with escaped characters.
        let b = Base::parse("http://h/my%20share/").unwrap();
        assert_eq!(b.relative("/my share/x"), Some(vec!["x".to_owned()]));
    }

    #[test]
    fn hrefs_after_a_redirect() {
        let b = Base::parse("http://h/old/").unwrap();
        let moved = Url::parse("http://h/new/root/a/b/").unwrap();
        let seen = b.as_seen_by(&moved, &p("/a/b"));
        assert_eq!(seen.relative("/new/root/a/b/c"), Some(vec!["a".into(), "b".into(), "c".into()]));
        // No redirect: unchanged.
        let same = b.as_seen_by(&Url::parse("http://h/old/a").unwrap(), &p("/a"));
        assert_eq!(same.relative("/old/a"), Some(vec!["a".to_owned()]));
        // Unrelated target: the configured root.
        let other = b.as_seen_by(&Url::parse("http://h/x/y").unwrap(), &p("/a"));
        assert_eq!(other.relative("/old/a"), Some(vec!["a".to_owned()]));
    }
}
