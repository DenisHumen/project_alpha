// SPDX-License-Identifier: MPL-2.0
//! The one runtime of the process, for the FFI facade.
//!
//! Apps call `init` on start, `shutdown` on exit, and every other FFI call
//! finds the runtime with [`current`]. `init` after `shutdown` starts a
//! fresh runtime (Android may recreate the app process state, tests start
//! and stop the core many times).

use std::sync::{Mutex, MutexGuard};

use crate::error::RuntimeError;
use crate::runtime::{Runtime, RuntimeConfig};

static CURRENT: Mutex<Option<Runtime>> = Mutex::new(None);

fn slot() -> MutexGuard<'static, Option<Runtime>> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Starts the process runtime.
///
/// Fails with [`RuntimeError::AlreadyInitialized`] while a runtime is
/// running; a runtime that was shut down is replaced.
pub fn init(config: RuntimeConfig) -> Result<Runtime, RuntimeError> {
    let mut slot = slot();
    if slot.as_ref().is_some_and(|rt| !rt.is_shut_down()) {
        return Err(RuntimeError::AlreadyInitialized);
    }
    let runtime = Runtime::start(config)?;
    *slot = Some(runtime.clone());
    Ok(runtime)
}

/// The running process runtime, or [`RuntimeError::NotInitialized`].
pub fn current() -> Result<Runtime, RuntimeError> {
    slot()
        .as_ref()
        .filter(|rt| !rt.is_shut_down())
        .cloned()
        .ok_or(RuntimeError::NotInitialized)
}

/// Whether a runtime is running.
pub fn is_initialized() -> bool {
    current().is_ok()
}

/// Shuts the process runtime down and forgets it. Does nothing when no
/// runtime is running.
pub fn shutdown() -> Result<(), RuntimeError> {
    // Take it out first: the shutdown may wait for tasks, and other threads
    // must not block on the slot meanwhile.
    let runtime = slot().take();
    match runtime {
        Some(runtime) => runtime.shutdown(),
        None => Ok(()),
    }
}
