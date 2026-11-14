// SPDX-License-Identifier: MPL-2.0
//! Locating and loading the libsmb2 shared library.

use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use crate::error::Error;
use crate::sys::Functions;

/// Environment variable with an explicit path to libsmb2; it wins over the
/// platform names in [`default_names`].
pub const LIB_ENV: &str = "PA_SMB2_LIB";

/// A loaded libsmb2. Cheap to share: [`Smb2Lib::shared`] hands out one
/// `'static` instance per process; the library is never unloaded.
pub struct Smb2Lib {
    // Keeps the library mapped as long as `functions` may be called.
    _lib: libloading::Library,
    pub(crate) functions: Functions,
    origin: String,
}

impl fmt::Debug for Smb2Lib {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Smb2Lib")
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

/// File names tried by [`Smb2Lib::load_default`] on this platform, in order.
///
/// They match what `tools/libsmb2/build.sh` produces and how the clients
/// package it: next to the executable or on the loader path (Linux,
/// Windows), in the APK's `jniLibs` (Android), via `@rpath` in the app
/// bundle (macOS dylib, iOS and tvOS `smb2.framework`).
#[must_use]
pub fn default_names() -> &'static [&'static str] {
    if cfg!(target_os = "android") {
        &["libsmb2.so"]
    } else if cfg!(any(target_os = "ios", target_os = "tvos")) {
        &[
            "@rpath/smb2.framework/smb2",
            "@executable_path/Frameworks/smb2.framework/smb2",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "libsmb2.1.dylib",
            "@rpath/libsmb2.1.dylib",
            "@executable_path/../Frameworks/libsmb2.1.dylib",
            "libsmb2.dylib",
        ]
    } else if cfg!(windows) {
        &["libsmb2.dll", "smb2.dll"]
    } else {
        &["libsmb2.so.1", "libsmb2.so"]
    }
}

static SHARED: Mutex<Option<&'static Smb2Lib>> = Mutex::new(None);

impl Smb2Lib {
    /// Loads libsmb2 from `path` (a file path or a name the platform loader
    /// resolves) and resolves every function the wrapper uses.
    ///
    /// Fails with [`ErrorKind::LibraryUnavailable`](crate::ErrorKind::LibraryUnavailable)
    /// if the file cannot be loaded or a function is missing.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let origin = path.display().to_string();
        // SAFETY: loading runs the library's initialisers. libsmb2 has none
        // with side effects beyond its own globals.
        let lib = unsafe { libloading::Library::new(path) }
            .map_err(|e| Error::library(format!("cannot load {origin}: {e}")))?;
        // SAFETY: the file is expected to be libsmb2 6.x; a library that
        // lacks one of the functions is rejected here.
        let functions = unsafe { Functions::load(&lib) }
            .map_err(|e| Error::library(format!("{origin} is not a usable libsmb2: {e}")))?;
        Ok(Self {
            _lib: lib,
            functions,
            origin,
        })
    }

    /// Loads libsmb2 from `$PA_SMB2_LIB` or, if unset, the first of
    /// [`default_names`] the platform loader finds.
    pub fn load_default() -> Result<Self, Error> {
        if let Some(path) = std::env::var_os(LIB_ENV).filter(|p| !p.is_empty()) {
            return Self::load(path);
        }
        let mut errors = Vec::new();
        for name in default_names() {
            match Self::load(OsString::from(name)) {
                Ok(lib) => return Ok(lib),
                Err(e) => errors.push(e.message().to_owned()),
            }
        }
        Err(Error::library(format!(
            "libsmb2 not found (set {LIB_ENV}): {}",
            errors.join("; ")
        )))
    }

    /// The process-wide library, loaded with [`load_default`](Self::load_default)
    /// on first success. A failure is not cached: a later call retries.
    pub fn shared() -> Result<&'static Self, Error> {
        let mut slot = SHARED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(lib) = *slot {
            return Ok(lib);
        }
        let lib: &'static Self = Box::leak(Box::new(Self::load_default()?));
        *slot = Some(lib);
        Ok(lib)
    }

    /// Where the library was loaded from (the path or name passed to the loader).
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }
}
