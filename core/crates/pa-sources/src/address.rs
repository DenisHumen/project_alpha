// SPDX-License-Identifier: MPL-2.0
//! Addresses of sources: validation and normalization to the `sources.uri`
//! form, which never holds credentials.
//!
//! | Kind | Accepted | Stored |
//! |---|---|---|
//! | `local` | absolute path, `file://` URI | `file:///…` (percent-encoded) |
//! | `smb` | `smb://host[:port]/share[/folder]` | as given, slashes normalized |
//! | `webdav` | `http(s)://…`, `webdav(s)://…`, `davs://…` | `http(s)://…` |
//!
//! The `file://` encoding is the same as in `pa-catalog`, so sources added
//! by either crate are the same rows.

use std::path::{Path, PathBuf};

use crate::{Error, Result, SourceKind};

/// Error text for `user:pass@host` addresses. It must not echo the address:
/// the password is in it.
const CREDENTIALS_IN_ADDRESS: &str =
    "the address must not contain a user name or password; use the login fields";

/// Normalizes `address` of a source of `kind` to the stored URI.
pub(crate) fn normalize(kind: SourceKind, address: &str) -> Result<String> {
    let address = address.trim();
    if address.is_empty() {
        return Err(Error::Invalid("empty address".into()));
    }
    match kind {
        SourceKind::Local => {
            let path = if address.starts_with("file://") {
                file_uri_to_path(address)
                    .ok_or_else(|| Error::Invalid(format!("malformed file URI: {address}")))?
            } else {
                PathBuf::from(address)
            };
            if !is_absolute(&path) {
                return Err(Error::Invalid(format!("not an absolute path: {address}")));
            }
            Ok(path_to_file_uri(&path))
        }
        SourceKind::Smb => SmbAddress::parse(address).map(|a| a.to_uri()),
        SourceKind::WebDav => {
            let (scheme, rest) = address
                .split_once("://")
                .ok_or_else(|| Error::Invalid("WebDAV address needs http:// or https://".into()))?;
            let scheme = match scheme.to_ascii_lowercase().as_str() {
                "http" | "webdav" => "http",
                "https" | "webdavs" | "davs" => "https",
                _ => {
                    return Err(Error::Invalid(
                        "WebDAV address needs http:// or https://".into(),
                    ));
                }
            };
            let authority = authority(rest)?;
            if authority.is_empty() {
                return Err(Error::Invalid("WebDAV address has no host".into()));
            }
            Ok(format!("{scheme}://{rest}"))
        }
    }
}

/// `C:/x` counts as absolute on every platform: the database is shared.
fn is_absolute(path: &Path) -> bool {
    let s = path.to_string_lossy();
    let b = s.as_bytes();
    path.is_absolute()
        || s.starts_with('/')
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'/' | b'\\'))
}

/// The authority part of `rest` (after `scheme://`); refuses user info.
fn authority(rest: &str) -> Result<&str> {
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    if authority.contains('@') {
        return Err(Error::Invalid(CREDENTIALS_IN_ADDRESS.into()));
    }
    Ok(authority)
}

/// Parts of an `smb://host[:port]/share[/folder]` address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmbAddress {
    /// `host`, `host:port` or `[ipv6]:port`.
    pub server: String,
    /// Share name.
    pub share: String,
    /// Folder inside the share, `/`-separated, without leading or trailing
    /// slashes; empty for the share root.
    pub folder: String,
}

impl SmbAddress {
    /// Parses an `smb://` address. Refuses user info, queries and fragments.
    pub fn parse(address: &str) -> Result<Self> {
        let rest = address
            .get(..6)
            .filter(|s| s.eq_ignore_ascii_case("smb://"))
            .map(|_| &address[6..])
            .ok_or_else(|| Error::Invalid("SMB address needs smb://host/share".into()))?;
        let server = authority(rest)?;
        if rest.contains(['?', '#']) {
            return Err(Error::Invalid(
                "SMB address must not have a query or fragment".into(),
            ));
        }
        let mut parts = rest[server.len()..]
            .split(['/', '\\'])
            .filter(|p| !p.is_empty());
        let share = parts.next().unwrap_or_default();
        if server.is_empty() || share.is_empty() {
            return Err(Error::Invalid("SMB address needs smb://host/share".into()));
        }
        Ok(Self {
            server: server.to_owned(),
            share: share.to_owned(),
            folder: parts.collect::<Vec<_>>().join("/"),
        })
    }

    /// `smb://server/share[/folder]`.
    pub fn to_uri(&self) -> String {
        let mut uri = format!("smb://{}/{}", self.server, self.share);
        if !self.folder.is_empty() {
            uri.push('/');
            uri.push_str(&self.folder);
        }
        uri
    }
}

/// `file://` URI of an absolute local path (percent-encoded outside the
/// unreserved set, `/` and `:`).
pub fn path_to_file_uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        // `C:/Media` → `file:///C:/Media`.
        out.push('/');
    }
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
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

    fn ok(kind: SourceKind, address: &str) -> String {
        normalize(kind, address).unwrap()
    }

    fn invalid(kind: SourceKind, address: &str) -> String {
        match normalize(kind, address) {
            Err(Error::Invalid(m)) => m,
            other => panic!("{address}: {other:?}"),
        }
    }

    #[test]
    fn local_paths() {
        assert_eq!(
            ok(SourceKind::Local, "/mnt/My Movies"),
            "file:///mnt/My%20Movies"
        );
        assert_eq!(ok(SourceKind::Local, "file:///mnt/a"), "file:///mnt/a");
        assert_eq!(ok(SourceKind::Local, "C:\\Media"), "file:///C:/Media");
        assert_eq!(
            file_uri_to_path(&ok(SourceKind::Local, "/home/x/Фильмы")).unwrap(),
            PathBuf::from("/home/x/Фильмы")
        );
        invalid(SourceKind::Local, "relative/dir");
        invalid(SourceKind::Local, "  ");
    }

    #[test]
    fn smb_addresses() {
        assert_eq!(
            ok(SourceKind::Smb, "smb://nas.local/media//Movies/4K/"),
            "smb://nas.local/media/Movies/4K"
        );
        let a = SmbAddress::parse("SMB://[fe80::1]:4445/share").unwrap();
        assert_eq!(
            (a.server.as_str(), a.share.as_str(), a.folder.as_str()),
            ("[fe80::1]:4445", "share", "")
        );
        invalid(SourceKind::Smb, "smb://nas");
        invalid(SourceKind::Smb, "//nas/share");
        invalid(SourceKind::Smb, "smb://nas/share?x=1");
    }

    #[test]
    fn webdav_addresses() {
        assert_eq!(
            ok(SourceKind::WebDav, "davs://cloud.example/remote.php/dav/"),
            "https://cloud.example/remote.php/dav/"
        );
        assert_eq!(ok(SourceKind::WebDav, "http://nas:8080"), "http://nas:8080");
        invalid(SourceKind::WebDav, "ftp://nas/");
        invalid(SourceKind::WebDav, "https:///x");
    }

    #[test]
    fn credentials_in_addresses_are_refused_without_echo() {
        for (kind, address) in [
            (SourceKind::Smb, "smb://anna:Pa55word@nas/media"),
            (
                SourceKind::WebDav,
                "https://anna:Pa55word@cloud.example/dav/",
            ),
            (SourceKind::Smb, "smb://anna@nas/media"),
        ] {
            let message = invalid(kind, address);
            assert!(!message.contains("Pa55word"), "{message}");
            assert_eq!(message, CREDENTIALS_IN_ADDRESS);
        }
    }
}
