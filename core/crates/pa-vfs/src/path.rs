// SPDX-License-Identifier: MPL-2.0
//! Protocol-neutral paths inside a source.

use std::fmt;

use crate::error::{ErrorKind, Result, VfsError};

/// Absolute, normalized path inside a source, relative to the source root.
///
/// The same type addresses a local folder, an SMB share, an NFS export, a
/// WebDAV collection and an SFTP directory, so it follows the common
/// denominator of those protocols:
///
/// - UTF-8, `/` as the separator, always starts with `/`; the root is `/`.
/// - No empty, `.` or `..` components, no trailing `/` (except the root),
///   no NUL. A path therefore can never point above the source root.
/// - Case and Unicode normalization are kept as given: the backend decides
///   whether `A.mkv` and `a.mkv` are the same file.
///
/// Backend-specific restrictions (for example `\` or `:` on Windows and SMB)
/// are checked by the backend and reported as [`ErrorKind::InvalidPath`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VfsPath(String);

impl VfsPath {
    /// The source root, `/`.
    #[must_use]
    pub fn root() -> Self {
        Self("/".to_owned())
    }

    /// Parses and normalizes a path.
    ///
    /// Accepts paths with or without the leading `/`, repeated `/` and `.`
    /// components (`a//b/./c` → `/a/b/c`). Rejects `..`, NUL and
    /// non-UTF-8 input with [`ErrorKind::InvalidPath`].
    pub fn parse(path: &str) -> Result<Self> {
        let mut out = String::with_capacity(path.len() + 1);
        for component in path.split('/') {
            match component {
                "" | "." => {}
                ".." => return Err(invalid(path, "`..` is not allowed")),
                name => {
                    check_name(name).map_err(|why| invalid(path, why))?;
                    out.push('/');
                    out.push_str(name);
                }
            }
        }
        if out.is_empty() {
            out.push('/');
        }
        Ok(Self(out))
    }

    /// Appends one file or directory name.
    ///
    /// `name` must be a single component: no `/`, not empty, not `.` or `..`.
    pub fn join(&self, name: &str) -> Result<Self> {
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(invalid(name, "not a single path component"));
        }
        check_name(name).map_err(|why| invalid(name, why))?;
        let mut out = String::with_capacity(self.0.len() + 1 + name.len());
        out.push_str(&self.0);
        if !self.is_root() {
            out.push('/');
        }
        out.push_str(name);
        Ok(Self(out))
    }

    /// `true` for `/`.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }

    /// The parent directory; `None` for the root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return None;
        }
        let cut = self.0.rfind('/').unwrap_or(0);
        Some(if cut == 0 {
            Self::root()
        } else {
            Self(self.0[..cut].to_owned())
        })
    }

    /// The last component; `None` for the root.
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        if self.is_root() {
            None
        } else {
            self.0.rsplit('/').next()
        }
    }

    /// The components after the root, in order (`/a/b` → `a`, `b`).
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|c| !c.is_empty())
    }

    /// The path as a string, always starting with `/`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for VfsPath {
    fn default() -> Self {
        Self::root()
    }
}

impl fmt::Display for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for VfsPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for VfsPath {
    type Err = VfsError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

fn check_name(name: &str) -> std::result::Result<(), &'static str> {
    if name.contains('\0') {
        return Err("NUL is not allowed");
    }
    Ok(())
}

fn invalid(input: &str, why: &str) -> VfsError {
    VfsError::new(ErrorKind::InvalidPath, format!("{input:?}: {why}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> VfsPath {
        VfsPath::parse(s).unwrap()
    }

    #[test]
    fn parse_normalizes() {
        assert_eq!(p("").as_str(), "/");
        assert_eq!(p("/").as_str(), "/");
        assert_eq!(p("//./").as_str(), "/");
        assert_eq!(p("a/b").as_str(), "/a/b");
        assert_eq!(p("/a//b/./c/").as_str(), "/a/b/c");
        assert_eq!(
            p("/Фильмы/Дюна (2021).mkv").as_str(),
            "/Фильмы/Дюна (2021).mkv"
        );
        assert_eq!("/x".parse::<VfsPath>().unwrap(), p("x"));
    }

    #[test]
    fn parse_rejects_escape_and_nul() {
        for bad in ["..", "/a/../b", "a/..", "/a\0b"] {
            let err = VfsPath::parse(bad).unwrap_err();
            assert_eq!(err.kind(), ErrorKind::InvalidPath, "{bad:?}");
        }
    }

    #[test]
    fn join_parent_file_name() {
        let root = VfsPath::root();
        let a = root.join("a").unwrap();
        let ab = a.join("b c.mkv").unwrap();
        assert_eq!(a.as_str(), "/a");
        assert_eq!(ab.as_str(), "/a/b c.mkv");
        assert_eq!(ab.parent(), Some(a.clone()));
        assert_eq!(a.parent(), Some(root.clone()));
        assert_eq!(root.parent(), None);
        assert_eq!(ab.file_name(), Some("b c.mkv"));
        assert_eq!(root.file_name(), None);
        assert_eq!(ab.components().collect::<Vec<_>>(), ["a", "b c.mkv"]);
        assert_eq!(root.components().count(), 0);
        for bad in ["", ".", "..", "a/b", "a\0"] {
            assert_eq!(
                root.join(bad).unwrap_err().kind(),
                ErrorKind::InvalidPath,
                "{bad:?}"
            );
        }
    }
}
