// SPDX-License-Identifier: MPL-2.0
//! Locating and loading the FFmpeg shared libraries.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::{ProbeError, ProbeErrorKind};
use crate::sys::{self, Functions, Libraries};

/// Environment variable with the directory holding the FFmpeg shared
/// libraries (e.g. `tools/ffmpeg/out/linux-x86_64/lib`); it wins over the
/// platform loader search.
pub const LIB_DIR_ENV: &str = "PA_FFMPEG_LIB_DIR";

/// One FFmpeg release line: majors of libavutil, libswresample, libavcodec
/// and libavformat. Tried newest first.
#[derive(Debug, Clone, Copy)]
struct Family {
    avutil: u32,
    swresample: u32,
    avcodec: u32,
    avformat: u32,
}

const FAMILIES: [Family; 3] = [
    // FFmpeg 9.x: our tools/ffmpeg build (PLAY-001).
    Family {
        avutil: 61,
        swresample: 7,
        avcodec: 63,
        avformat: 63,
    },
    // FFmpeg 8.x.
    Family {
        avutil: 60,
        swresample: 6,
        avcodec: 62,
        avformat: 62,
    },
    // FFmpeg 7.x (Debian 13, Ubuntu 24.10+ system libraries).
    Family {
        avutil: 59,
        swresample: 5,
        avcodec: 61,
        avformat: 61,
    },
];

/// File name of FFmpeg library `name` (`avutil`, `avformat`…) with major
/// `major` on this platform, as `tools/ffmpeg` produces it and the clients
/// package it.
#[must_use]
pub fn file_name(name: &str, major: u32) -> String {
    if cfg!(target_os = "android") {
        // jniLibs keep only `lib*.so` (media_kit packages them this way).
        format!("lib{name}.so")
    } else if cfg!(target_vendor = "apple") {
        format!("lib{name}.{major}.dylib")
    } else if cfg!(windows) {
        format!("{name}-{major}.dll")
    } else {
        format!("lib{name}.so.{major}")
    }
}

/// Loaded FFmpeg libraries and the functions the prober uses. Cheap to
/// share: [`Ffmpeg::shared`] hands out one `'static` instance per process;
/// the libraries are never unloaded.
pub struct Ffmpeg {
    // Keep the libraries mapped as long as `functions` may be called.
    _libs: Vec<libloading::Library>,
    pub(crate) functions: Functions,
    origin: String,
    versions: [u32; 3],
}

impl fmt::Debug for Ffmpeg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ffmpeg")
            .field("origin", &self.origin)
            .field("versions", &self.versions)
            .finish_non_exhaustive()
    }
}

static SHARED: Mutex<Option<&'static Ffmpeg>> = Mutex::new(None);

impl Ffmpeg {
    /// Loads FFmpeg from directory `dir`, trying each supported release
    /// line (newest first).
    ///
    /// # Errors
    ///
    /// [`ProbeErrorKind::LibraryUnavailable`] if no supported set of
    /// libraries loads from `dir`.
    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self, ProbeError> {
        let dir = dir.as_ref();
        Self::load_with(&dir.display().to_string(), |name| dir.join(name))
    }

    /// Loads FFmpeg from `$PA_FFMPEG_LIB_DIR` or, if unset, by file name
    /// through the platform loader (next to the executable, `LD_LIBRARY_PATH`,
    /// APK `jniLibs`, `@rpath`…).
    ///
    /// # Errors
    ///
    /// [`ProbeErrorKind::LibraryUnavailable`] if FFmpeg is not found or is
    /// not a supported version.
    pub fn load_default() -> Result<Self, ProbeError> {
        match std::env::var_os(LIB_DIR_ENV).filter(|p| !p.is_empty()) {
            Some(dir) => Self::load_from_dir(PathBuf::from(dir)),
            None => Self::load_with("system loader", PathBuf::from),
        }
    }

    /// The process-wide instance, loaded with [`load_default`](Self::load_default)
    /// on first success. A failure is not cached: a later call retries.
    ///
    /// # Errors
    ///
    /// As [`load_default`](Self::load_default).
    pub fn shared() -> Result<&'static Self, ProbeError> {
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

    /// Where the libraries were loaded from.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Raw `avutil_version()`, `avcodec_version()`, `avformat_version()`
    /// (`major << 16 | minor << 8 | micro`).
    #[must_use]
    pub fn versions(&self) -> [u32; 3] {
        self.versions
    }

    fn load_with(origin: &str, path: impl Fn(&str) -> PathBuf) -> Result<Self, ProbeError> {
        let mut errors = Vec::new();
        for family in FAMILIES {
            match Self::load_family(family, &path) {
                Ok((libs, functions)) => {
                    return Self::checked(libs, functions, origin.to_owned());
                }
                Err(e) => errors.push(e),
            }
        }
        Err(ProbeError::new(
            ProbeErrorKind::LibraryUnavailable,
            format!(
                "FFmpeg not found in {origin} (set {LIB_DIR_ENV}): {}",
                errors.join("; ")
            ),
        ))
    }

    fn load_family(
        family: Family,
        path: &impl Fn(&str) -> PathBuf,
    ) -> Result<(Vec<libloading::Library>, Functions), String> {
        // Dependencies first: when a library is opened by full path, its
        // DT_NEEDED entries are satisfied by the already loaded sonames.
        let order = [
            ("avutil", family.avutil),
            ("swresample", family.swresample),
            ("avcodec", family.avcodec),
            ("avformat", family.avformat),
        ];
        let mut libs = Vec::with_capacity(order.len());
        for (name, major) in order {
            let file = path(&file_name(name, major));
            // SAFETY: loading runs FFmpeg's initialisers, which only set up
            // its own static tables.
            match unsafe { libloading::Library::new(&file) } {
                Ok(lib) => libs.push(lib),
                // libswresample is only a dependency of libavcodec; a build
                // without it is fine.
                Err(_) if name == "swresample" => {}
                Err(e) => return Err(format!("{}: {e}", file.display())),
            }
        }
        let n = libs.len();
        let functions = {
            let set = Libraries {
                avutil: &libs[0],
                avcodec: &libs[n - 2],
                avformat: &libs[n - 1],
            };
            // SAFETY: the files are FFmpeg's libraries of one release line;
            // a library that lacks a function is rejected here.
            unsafe { Functions::load(&set) }?
        };
        Ok((libs, functions))
    }

    fn checked(
        libs: Vec<libloading::Library>,
        functions: Functions,
        origin: String,
    ) -> Result<Self, ProbeError> {
        // SAFETY: plain getters without arguments.
        let versions = unsafe {
            [
                (functions.avutil_version)(),
                (functions.avcodec_version)(),
                (functions.avformat_version)(),
            ]
        };
        let [util, codec, format] = versions.map(|v| v >> 16);
        if !sys::SUPPORTED_AVUTIL.contains(&util)
            || !sys::SUPPORTED_AVCODEC.contains(&codec)
            || !sys::SUPPORTED_AVFORMAT.contains(&format)
            || codec != format
        {
            return Err(ProbeError::new(
                ProbeErrorKind::LibraryUnavailable,
                format!(
                    "unsupported FFmpeg in {origin}: avutil {util}, avcodec {codec}, avformat {format}"
                ),
            ));
        }
        Ok(Self {
            _libs: libs,
            functions,
            origin,
            versions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_per_platform() {
        let name = file_name("avformat", 63);
        if cfg!(target_os = "linux") {
            assert_eq!(name, "libavformat.so.63");
        }
        assert!(name.contains("avformat"));
    }

    #[test]
    fn missing_directory_is_library_unavailable() {
        let err = Ffmpeg::load_from_dir("/nonexistent/pa-probe-test").unwrap_err();
        assert_eq!(err.kind(), ProbeErrorKind::LibraryUnavailable);
        assert!(err.to_string().contains("FFmpeg not found"));
    }
}
