// SPDX-License-Identifier: MPL-2.0
//! `file://` URIs of local source roots (`sources.uri`).
//!
//! Only what a source root needs: an absolute POSIX path or a Windows drive
//! path, percent-encoded outside the unreserved set and `/`.

use std::path::{Path, PathBuf};

/// `file://` URI of an absolute local path.
pub fn path_to_file_uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        // `C:/Media` → `file:///C:/Media`.
        out.push('/');
    }
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric() || b"-._~/:".contains(&b);
        if keep {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Local path of a `file://` URI; `None` for other schemes or a malformed
/// escape.
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Drop an authority (`file://localhost/…`).
    let rest = if rest.starts_with('/') {
        rest
    } else {
        &rest[rest.find('/')?..]
    };
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = rest.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let mut s = String::from_utf8(out).ok()?;
    // `/C:/Media` → `C:/Media`.
    if s.len() >= 3 && s.as_bytes()[0] == b'/' && s.as_bytes()[2] == b':' {
        s.remove(0);
    }
    Some(PathBuf::from(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for p in ["/home/user/Фильмы/My Movies", "/a/b%c#d?e", "/"] {
            let uri = path_to_file_uri(Path::new(p));
            assert!(uri.starts_with("file:///"), "{uri}");
            assert_eq!(file_uri_to_path(&uri).unwrap(), PathBuf::from(p));
        }
    }

    #[test]
    fn spaces_are_escaped() {
        assert_eq!(
            path_to_file_uri(Path::new("/mnt/My Movies")),
            "file:///mnt/My%20Movies"
        );
    }

    #[test]
    fn other_schemes_have_no_path() {
        assert_eq!(file_uri_to_path("smb://nas/share"), None);
        assert_eq!(
            file_uri_to_path("file://localhost/srv/a").unwrap(),
            PathBuf::from("/srv/a")
        );
    }
}
