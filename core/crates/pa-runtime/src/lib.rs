// SPDX-License-Identifier: MPL-2.0
//! Core runtime of project_alpha: lifecycle, data directories and settings.
//!
//! One [`Runtime`] owns everything that lives as long as the core is loaded
//! in the app process:
//!
//! - a multi-threaded tokio runtime for all async work of the core;
//! - the [`LifecycleState`] (`Foreground`, `Background`, `ShutDown`), which
//!   the app reports and other subsystems watch with [`Runtime::subscribe`];
//! - the per-platform data directories ([`PlatformDirs`]);
//! - the [`Settings`] key-value store (SQLite with migrations, JSON export and
//!   import).
//!
//! Lifecycle: [`Runtime::start`] → any number of
//! [`Runtime::enter_background`] / [`Runtime::enter_foreground`] →
//! [`Runtime::shutdown`]. After shutdown a new runtime can be started again
//! in the same process, with the same directories; settings written before
//! are there.
//!
//! The FFI facade (`pa-ffi`) keeps exactly one runtime per process through
//! the [`global`] module. The platform talks back to the core through the
//! [`PlatformHost`] callback interface.
//!
//! Settings hold user preferences only. Secrets (passwords, tokens) never go
//! here: they belong to `pa-secrets` (CORE-007), because settings are exported
//! to a plain JSON file.

mod error;
pub mod global;
mod host;
mod paths;
mod runtime;
mod settings;

pub use error::{RuntimeError, SettingsError};
pub use host::{LifecycleState, PlatformHost};
pub use paths::{DEFAULT_APP_DIR_NAME, Platform, PlatformDirs};
pub use runtime::{Runtime, RuntimeConfig};
pub use settings::{
    EXPORT_FORMAT, EXPORT_VERSION, ImportMode, MAX_SETTING_KEY_LEN, SCHEMA_VERSION, SETTINGS_FILE,
    Settings, SettingsExport,
};
