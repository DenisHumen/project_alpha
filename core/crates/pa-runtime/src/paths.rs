// SPDX-License-Identifier: MPL-2.0
//! Platforms and their data directories.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::RuntimeError;

/// Name of the per-app directory inside the platform's data, config and
/// cache locations. On Linux it is lower-cased (`kinolune`), following XDG
/// habits. The product name is still a working name (ADR-0011, DEC-004), so
/// it lives in this one constant.
pub const DEFAULT_APP_DIR_NAME: &str = "Kinolune";

/// Operating system the core runs on. It decides the default directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Platform {
    /// Linux desktop or the media server (XDG directories).
    Linux,
    /// Windows desktop.
    Windows,
    /// macOS.
    MacOs,
    /// iOS and iPadOS.
    Ios,
    /// tvOS: only Application Support persists; Caches may be purged.
    TvOs,
    /// Android and Android TV: directories come from the app.
    Android,
    /// Any other OS (BSDs): treated like Linux.
    Other,
}

impl Platform {
    /// Platform of the current build target.
    pub const fn current() -> Self {
        if cfg!(target_os = "android") {
            Self::Android
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "tvos") {
            Self::TvOs
        } else if cfg!(target_os = "ios") {
            Self::Ios
        } else {
            Self::Other
        }
    }

    /// Stable lower-case name (`linux`, `windows`, `macos`, `ios`, `tvos`,
    /// `android`, `other`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Ios => "ios",
            Self::TvOs => "tvos",
            Self::Android => "android",
            Self::Other => "other",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The three directories the core writes to. All paths are absolute.
///
/// - `data`: databases and other state that must persist (library, jobs).
/// - `config`: the settings database ([`SETTINGS_FILE`](crate::SETTINGS_FILE)).
///   May be the same directory as `data`.
/// - `cache`: artwork and other files the OS may delete at any time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformDirs {
    /// Persistent state.
    pub data: PathBuf,
    /// Settings.
    pub config: PathBuf,
    /// Disposable cache.
    pub cache: PathBuf,
}

impl PlatformDirs {
    /// Directories given explicitly by the app (Android passes
    /// `Context.filesDir` and `Context.cacheDir`).
    pub fn new(
        data: impl Into<PathBuf>,
        config: impl Into<PathBuf>,
        cache: impl Into<PathBuf>,
    ) -> Self {
        Self {
            data: data.into(),
            config: config.into(),
            cache: cache.into(),
        }
    }

    /// Layout inside an Apple app container (iOS, iPadOS, tvOS): data and
    /// settings in `Library/Application Support/<app>`, cache in
    /// `Library/Caches/<app>`.
    ///
    /// On tvOS this is the only correct choice: there is no `Documents`, and
    /// the system may purge `Caches` when space is low, so nothing that must
    /// survive goes there.
    pub fn apple_container(home: &Path, app_dir_name: &str) -> Self {
        let library = home.join("Library");
        let data = library.join("Application Support").join(app_dir_name);
        Self {
            config: data.clone(),
            data,
            cache: library.join("Caches").join(app_dir_name),
        }
    }

    /// Default directories of `platform`.
    ///
    /// | Platform | data | config | cache |
    /// |---|---|---|---|
    /// | Linux, other | `$XDG_DATA_HOME/<app>` | `$XDG_CONFIG_HOME/<app>` | `$XDG_CACHE_HOME/<app>` |
    /// | macOS | `~/Library/Application Support/<app>` | same as data | `~/Library/Caches/<app>` |
    /// | Windows | `%LOCALAPPDATA%\<app>` | `%APPDATA%\<app>` | `%LOCALAPPDATA%\<app>\cache` |
    /// | iOS, tvOS | [`apple_container`](Self::apple_container) of `$HOME` | | |
    /// | Android | none: [`RuntimeError::DirsRequired`] | | |
    ///
    /// On Linux `<app>` is lower-cased.
    pub fn platform_default(platform: Platform, app_dir_name: &str) -> Result<Self, RuntimeError> {
        let required = || RuntimeError::DirsRequired { platform };
        let dirs = match platform {
            Platform::Android => return Err(required()),
            Platform::Ios | Platform::TvOs => {
                Self::apple_container(&::dirs::home_dir().ok_or_else(required)?, app_dir_name)
            }
            Platform::MacOs => {
                let data = ::dirs::data_dir().ok_or_else(required)?.join(app_dir_name);
                Self {
                    config: data.clone(),
                    data,
                    cache: ::dirs::cache_dir().ok_or_else(required)?.join(app_dir_name),
                }
            }
            Platform::Windows => {
                let local = ::dirs::data_local_dir()
                    .ok_or_else(required)?
                    .join(app_dir_name);
                Self {
                    cache: local.join("cache"),
                    data: local,
                    config: ::dirs::config_dir()
                        .ok_or_else(required)?
                        .join(app_dir_name),
                }
            }
            Platform::Linux | Platform::Other => {
                let name = app_dir_name.to_lowercase();
                Self {
                    data: ::dirs::data_dir().ok_or_else(required)?.join(&name),
                    config: ::dirs::config_dir().ok_or_else(required)?.join(&name),
                    cache: ::dirs::cache_dir().ok_or_else(required)?.join(&name),
                }
            }
        };
        Ok(dirs)
    }

    /// Checks that every path is absolute.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        for path in [&self.data, &self.config, &self.cache] {
            if !path.is_absolute() {
                return Err(RuntimeError::RelativeDir { path: path.clone() });
            }
        }
        Ok(())
    }

    /// Validates the paths and creates the directories that do not exist.
    pub fn create_all(&self) -> Result<(), RuntimeError> {
        self.validate()?;
        for path in [&self.data, &self.config, &self.cache] {
            std::fs::create_dir_all(path).map_err(|source| RuntimeError::CreateDir {
                path: path.clone(),
                source,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_container_keeps_data_out_of_caches() {
        let home = Path::new("/container");
        let dirs = PlatformDirs::apple_container(home, "Kinolune");
        assert_eq!(
            dirs.data,
            Path::new("/container/Library/Application Support/Kinolune")
        );
        assert_eq!(dirs.config, dirs.data);
        assert_eq!(dirs.cache, Path::new("/container/Library/Caches/Kinolune"));
    }

    #[test]
    fn android_has_no_defaults() {
        let err = PlatformDirs::platform_default(Platform::Android, DEFAULT_APP_DIR_NAME)
            .expect_err("Android must pass directories");
        assert!(matches!(
            err,
            RuntimeError::DirsRequired {
                platform: Platform::Android
            }
        ));
    }

    #[test]
    fn tvos_default_is_apple_container_of_home() {
        let Some(home) = ::dirs::home_dir() else {
            return;
        };
        let dirs = PlatformDirs::platform_default(Platform::TvOs, "Kinolune").unwrap();
        assert_eq!(dirs, PlatformDirs::apple_container(&home, "Kinolune"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_defaults_are_absolute_and_lower_case() {
        let dirs = PlatformDirs::platform_default(Platform::Linux, "Kinolune").unwrap();
        dirs.validate().unwrap();
        for path in [&dirs.data, &dirs.config, &dirs.cache] {
            assert!(path.ends_with("kinolune"), "{path:?}");
        }
    }

    #[test]
    fn relative_dirs_are_rejected() {
        let dirs = PlatformDirs::new("relative/data", "/abs/config", "/abs/cache");
        assert!(matches!(
            dirs.validate(),
            Err(RuntimeError::RelativeDir { .. })
        ));
    }

    #[test]
    fn current_platform_matches_target() {
        if cfg!(target_os = "linux") {
            assert_eq!(Platform::current(), Platform::Linux);
        }
        assert_eq!(Platform::TvOs.to_string(), "tvos");
    }
}
