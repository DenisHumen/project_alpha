// SPDX-License-Identifier: MPL-2.0
//! Lifecycle states and the callback interface the platform implements.

use crate::paths::PlatformDirs;

/// Where the app is in its lifecycle, as seen by the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleState {
    /// The app is visible; all work may run.
    Foreground,
    /// The app is in the background: the OS may suspend or kill it at any
    /// moment. Subsystems finish or pause their work and keep state on disk.
    Background,
    /// [`Runtime::shutdown`](crate::Runtime::shutdown) ran; the instance is
    /// unusable. This is the last state a subscriber sees.
    ShutDown,
}

/// Callbacks from the core to the platform (the app embedding the core).
///
/// Every method has a default, so a platform implements only what it needs.
/// Methods are called synchronously on a core thread and must return fast;
/// they must not call back into lifecycle methods of the runtime.
///
/// The FFI facade adapts a UniFFI callback interface implemented in Kotlin or
/// Swift to this trait.
pub trait PlatformHost: Send + Sync {
    /// Data directories of a sandboxed platform. Asked once at start when
    /// [`RuntimeConfig`](crate::RuntimeConfig) has no explicit directories;
    /// `None` means "use the platform defaults".
    fn dirs(&self) -> Option<PlatformDirs> {
        None
    }

    /// The lifecycle state changed (including the final
    /// [`LifecycleState::ShutDown`]).
    fn on_lifecycle_changed(&self, state: LifecycleState) {
        let _ = state;
    }
}
