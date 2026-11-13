// SPDX-License-Identifier: MPL-2.0
//! Core lifecycle and settings over FFI (`pa-runtime`, CORE-006).
//!
//! The app calls [`core_init`] once on start, reports foreground and
//! background with [`core_enter_foreground`] / [`core_enter_background`], and
//! calls [`core_shutdown`] on exit. `core_init` after `core_shutdown` starts
//! the core again. Settings values cross the boundary as JSON text.

use std::path::PathBuf;
use std::sync::Arc;

use pa_runtime::{
    ImportMode, LifecycleState, Platform, PlatformDirs, PlatformHost, RuntimeConfig, RuntimeError,
    SettingsError, global,
};

/// Lifecycle state of the core as seen by the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CoreLifecycle {
    /// `core_init` was not called, or the core was shut down.
    NotInitialized,
    /// The app is visible.
    Foreground,
    /// The app is in the background.
    Background,
    /// The core is shutting down (sent to callbacks only).
    ShutDown,
}

impl From<LifecycleState> for CoreLifecycle {
    fn from(state: LifecycleState) -> Self {
        match state {
            LifecycleState::Foreground => Self::Foreground,
            LifecycleState::Background => Self::Background,
            LifecycleState::ShutDown => Self::ShutDown,
        }
    }
}

/// Absolute directories of the core.
///
/// Android passes `filesDir` (data and config) and `cacheDir`. On iOS and
/// tvOS the core defaults to `Library/Application Support` and
/// `Library/Caches` of the app container, so `dirs` may be left empty there.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoreDirs {
    /// Persistent state (databases).
    pub data: String,
    /// Settings database.
    pub config: String,
    /// Disposable cache.
    pub cache: String,
}

impl From<CoreDirs> for PlatformDirs {
    fn from(d: CoreDirs) -> Self {
        PlatformDirs::new(
            PathBuf::from(d.data),
            PathBuf::from(d.config),
            PathBuf::from(d.cache),
        )
    }
}

impl From<&PlatformDirs> for CoreDirs {
    fn from(d: &PlatformDirs) -> Self {
        Self {
            data: d.data.to_string_lossy().into_owned(),
            config: d.config.to_string_lossy().into_owned(),
            cache: d.cache.to_string_lossy().into_owned(),
        }
    }
}

/// Start options of the core.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct CoreConfig {
    /// Explicit directories; `None` uses the platform defaults (required on
    /// Android).
    pub dirs: Option<CoreDirs>,
    /// Number of worker threads; `None` means CPU count, at most 4.
    pub worker_threads: Option<u32>,
    /// Start in the background state (Android service, background fetch).
    pub start_in_background: bool,
}

/// Callbacks the app implements (Kotlin or Swift).
///
/// Called synchronously on a core thread: return fast and do not call
/// lifecycle functions of the core from inside.
#[uniffi::export(with_foreign)]
pub trait CoreCallbacks: Send + Sync {
    /// The lifecycle state changed (also sent once after `core_init` and
    /// once with `ShutDown`).
    fn on_lifecycle_changed(&self, state: CoreLifecycle);
}

struct HostAdapter(Arc<dyn CoreCallbacks>);

impl PlatformHost for HostAdapter {
    fn on_lifecycle_changed(&self, state: LifecycleState) {
        self.0.on_lifecycle_changed(state.into());
    }
}

/// Error of a core lifecycle or settings call.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    /// `core_init` while the core is running.
    #[error("the core is already initialized")]
    AlreadyInitialized,
    /// A call before `core_init` or after `core_shutdown`.
    #[error("the core is not initialized")]
    NotInitialized,
    /// Directories are missing, relative or cannot be created.
    #[error("data directories: {message}")]
    Dirs {
        /// Details for logs.
        message: String,
    },
    /// The settings key is empty, too long or not `[a-z0-9._-]`.
    #[error("invalid settings key {key:?}")]
    InvalidSettingsKey {
        /// The rejected key.
        key: String,
    },
    /// A value or an import document is not valid.
    #[error("invalid settings data: {message}")]
    InvalidSettingsData {
        /// Details for logs.
        message: String,
    },
    /// Database or runtime failure.
    #[error("internal error: {message}")]
    Internal {
        /// Details for logs.
        message: String,
    },
}

impl From<RuntimeError> for CoreError {
    fn from(err: RuntimeError) -> Self {
        match err {
            RuntimeError::AlreadyInitialized => Self::AlreadyInitialized,
            RuntimeError::NotInitialized | RuntimeError::ShutDown => Self::NotInitialized,
            RuntimeError::DirsRequired { .. }
            | RuntimeError::RelativeDir { .. }
            | RuntimeError::CreateDir { .. } => Self::Dirs {
                message: err.to_string(),
            },
            RuntimeError::Settings(err) => err.into(),
            other => Self::Internal {
                message: other.to_string(),
            },
        }
    }
}

impl From<SettingsError> for CoreError {
    fn from(err: SettingsError) -> Self {
        match err {
            SettingsError::InvalidKey(key) => Self::InvalidSettingsKey { key },
            SettingsError::Closed => Self::NotInitialized,
            SettingsError::InvalidExport(_) | SettingsError::Json(_) => Self::InvalidSettingsData {
                message: err.to_string(),
            },
            other => Self::Internal {
                message: other.to_string(),
            },
        }
    }
}

fn runtime() -> Result<pa_runtime::Runtime, CoreError> {
    Ok(global::current()?)
}

/// Starts the core: directories, settings database, worker threads.
#[uniffi::export]
pub fn core_init(
    config: CoreConfig,
    callbacks: Option<Arc<dyn CoreCallbacks>>,
) -> Result<(), CoreError> {
    let mut rt_config = RuntimeConfig {
        platform: Platform::current(),
        dirs: config.dirs.map(PlatformDirs::from),
        start_in_background: config.start_in_background,
        ..RuntimeConfig::default()
    };
    if let Some(threads) = config.worker_threads {
        rt_config = rt_config.with_worker_threads(threads as usize);
    }
    if let Some(callbacks) = callbacks {
        rt_config = rt_config.with_host(Arc::new(HostAdapter(callbacks)));
    }
    global::init(rt_config)?;
    Ok(())
}

/// Stops the core and closes its databases. Does nothing if not running.
#[uniffi::export]
pub fn core_shutdown() -> Result<(), CoreError> {
    Ok(global::shutdown()?)
}

/// The app became visible.
#[uniffi::export]
pub fn core_enter_foreground() -> Result<(), CoreError> {
    runtime()?.enter_foreground()?;
    Ok(())
}

/// The app went to the background; the core flushes its state to disk.
#[uniffi::export]
pub fn core_enter_background() -> Result<(), CoreError> {
    runtime()?.enter_background()?;
    Ok(())
}

/// Current lifecycle state.
#[uniffi::export]
pub fn core_lifecycle() -> CoreLifecycle {
    global::current().map_or(CoreLifecycle::NotInitialized, |rt| rt.state().into())
}

/// Directories the running core uses.
#[uniffi::export]
pub fn core_dirs() -> Result<CoreDirs, CoreError> {
    Ok(runtime()?.dirs().into())
}

/// Value of a setting as JSON text, `None` if it is not set.
#[uniffi::export]
pub fn settings_get(key: String) -> Result<Option<String>, CoreError> {
    let value = runtime()?.settings().get_value(&key)?;
    Ok(value.map(|v| v.to_string()))
}

/// Stores a setting; `value_json` is any JSON value (`"dark"`, `42`, `true`,
/// `{"size": 42}`).
#[uniffi::export]
pub fn settings_set(key: String, value_json: String) -> Result<(), CoreError> {
    let value: serde_json::Value =
        serde_json::from_str(&value_json).map_err(SettingsError::from)?;
    runtime()?.settings().set_value(&key, &value)?;
    Ok(())
}

/// Deletes a setting; returns whether it existed.
#[uniffi::export]
pub fn settings_remove(key: String) -> Result<bool, CoreError> {
    Ok(runtime()?.settings().remove(&key)?)
}

/// All setting keys, sorted.
#[uniffi::export]
pub fn settings_keys() -> Result<Vec<String>, CoreError> {
    Ok(runtime()?.settings().keys()?)
}

/// All settings as a JSON document for a backup file.
#[uniffi::export]
pub fn settings_export() -> Result<String, CoreError> {
    Ok(runtime()?.settings().export_json()?)
}

/// Imports a document made by [`settings_export`]. With `replace`, keys not
/// in the document are deleted; otherwise they are kept. All or nothing.
/// Returns the number of imported keys.
#[uniffi::export]
pub fn settings_import(json: String, replace: bool) -> Result<u32, CoreError> {
    let mode = if replace {
        ImportMode::Replace
    } else {
        ImportMode::Merge
    };
    let count = runtime()?.settings().import_json(&json, mode)?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}
